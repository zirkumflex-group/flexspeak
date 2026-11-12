//! The page with a collection server behind it: what each tap asks of the server
//! ([`Effect`]), and what each answer changes ([`Tracker::answer`]). Nothing here touches the
//! network or the disk, so tests feed in made-up answers; `main` runs the effects.

use super::{Can, FLAVOURS, FoundIdentity, Gate, Notice, Person, Ranking, Section, Tracker, api};
use api::{Problem, problem};

/// The app the identity offered at the last step comes from.
pub const OWN_SOURCE: &str = "Flexspeak";

/// Something for `main` to do: a call to the server, or keeping what one gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Keep this session for the next launch, or forget the kept one.
    Keep(Option<String>),
    /// Open this page in the system browser.
    Browse(String),
    /// Look up Flexspeak's own identity and hand it to `Tracker::found`.
    FindIdentity,
    Me,
    LoginStart(String),
    LoginFinish {
        email: String,
        code: String,
    },
    LoginPasskey,
    /// Ask, after a short pause, whether the passkey page is done.
    Poll {
        ticket: String,
        generation: u64,
    },
    /// Link Flexspeak's own identity: challenge, signature, link.
    Link,
    Cans,
    Add(api::AddCan),
    Remove(i64),
    Import(api::Import),
    Leaderboard(Ranking),
    Person(i64),
    Hide(bool),
    /// End this session on the server; the app has forgotten it already.
    SignOut(String),
}

/// What the server answered to an [`Effect`].
#[derive(Debug, Clone)]
pub enum Answer {
    Me(Result<api::Me, Problem>),
    /// A code was sent to this address.
    LoginStarted(String, Result<(), Problem>),
    LoginFinished(Result<api::LoginFinished, Problem>),
    Ticket(Result<api::Ticket, Problem>),
    Polled(u64, Result<api::Polled, Problem>),
    Linked(Result<api::Me, Problem>),
    Cans(Result<api::Cans, Problem>),
    Added(Result<api::Can, Problem>),
    Removed(i64, Result<(), Problem>),
    Imported(Result<api::Imported, Problem>),
    Leaderboard(Ranking, Result<api::Leaderboard, Problem>),
    Person(i64, Result<api::Collection, Problem>),
    Hidden(bool, Result<api::Me, Problem>),
}

/// A tap on one of the sign-in or account buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SendCode(String),
    SendAgain,
    OtherAddress,
    CheckCode(String),
    /// Sign in with a passkey made before, from the Email step.
    PasskeySignIn,
    OpenPasskey,
    CancelPasskey,
    /// Link the identity at this place in the Identity step's list.
    Link(usize),
    LinkAll,
    /// Leave the Identity step for the page, linked or not.
    Enter,
    SignOut,
}

/// Someone's board, opened from the leaderboard. `cans` is `None` until it arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub user: i64,
    pub name: String,
    pub cans: Option<Vec<Can>>,
    pub problem: Option<String>,
}

/// The collection server address typed in Settings: `None` when empty, which keeps the page
/// on this computer only.
pub fn server_address(typed: &str) -> Result<Option<String>, &'static str> {
    let address = typed.trim().trim_end_matches('/');
    if address.is_empty() {
        return Ok(None);
    }
    let rest = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"))
        .ok_or("Start the address with https://")?;
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return Err("That isn't a web address.");
    }
    Ok(Some(address.to_owned()))
}

/// A fresh key for one new can, so sending it again after a lost answer can't add two.
fn can_key() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Without randomness the time still tells two taps apart.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        bytes = nanos.to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn signed_out(problem: &Problem) -> bool {
    problem.error == problem::SIGNED_OUT
}

impl Tracker {
    /// Whether a collection server keeps the cans.
    pub fn online(&self) -> bool {
        self.server.is_some()
    }

    /// Switches to another server, or back to this computer only. Everything from the old one
    /// is dropped, the session with it; the page reads its collection again when it opens.
    pub fn set_server(&mut self, server: Option<String>) {
        if self.server == server {
            return;
        }
        self.leave();
        self.gate = None;
        self.session_read = false;
        self.cans.clear();
        self.db = None;
        self.key = None;
        self.load_error = None;
        self.local.clear();
        self.local_read = false;
        self.section = Section::Overview;
        self.server = server;
    }

    /// Forgets the session and everything it showed.
    fn leave(&mut self) {
        self.session = None;
        self.me = None;
        self.busy = false;
        self.gate_problem = None;
        self.poll += 1;
        self.loading = false;
        self.adding = false;
        self.people.clear();
        self.people_loading = false;
        self.people_problem = None;
        self.visit = None;
        self.hidden = false;
        self.notice = None;
        self.selected = None;
        self.reveal = None;
        if self.online() {
            self.cans.clear();
        }
        if matches!(self.section, Section::Leaderboard | Section::Person(_)) {
            self.section = Section::Overview;
        }
    }

    /// What opening the page asks of the server: with a session kept from before, who it
    /// belongs to and the collection; without one, signing in. `saved` is that session, read
    /// the first time only.
    pub fn connect(&mut self, saved: Option<String>) -> Vec<Effect> {
        if !self.online() {
            return Vec::new();
        }
        if !self.session_read {
            self.session_read = true;
            if self.session.is_none() {
                self.session = saved;
            }
        }
        if self.session.is_none() {
            if self.gate.is_none() {
                self.gate = Some(Gate::Email);
            }
            return Vec::new();
        }
        if self.gate.is_some() {
            // Halfway through the identity step: carry on there.
            return Vec::new();
        }
        self.loading = self.cans.is_empty();
        vec![Effect::Me, Effect::Cans]
    }

    /// Settings sync heard from the server that the session has ended: the page asks to sign
    /// in again, as it does when its own requests hear it.
    pub fn session_ended(&mut self) -> Vec<Effect> {
        if !self.online() || self.session.is_none() {
            return Vec::new();
        }
        self.ended()
    }

    /// The session ended on the server: sign in again.
    fn ended(&mut self) -> Vec<Effect> {
        let had = self.session.is_some();
        self.leave();
        self.gate = Some(Gate::Email);
        if had {
            self.gate_problem = Some("Your session has ended. Sign in again.".into());
            vec![Effect::Keep(None)]
        } else {
            Vec::new()
        }
    }

    /// A button on the sign-in steps or the account line.
    pub fn act(&mut self, action: Action) -> Vec<Effect> {
        if !self.online() {
            return Vec::new();
        }
        match action {
            Action::SendCode(email) => {
                let email = email.trim().to_owned();
                if self.busy || !email.contains('@') {
                    return Vec::new();
                }
                self.busy = true;
                self.gate_problem = None;
                vec![Effect::LoginStart(email)]
            }
            Action::SendAgain => match &self.gate {
                Some(Gate::Code { email, .. }) if !self.busy => {
                    self.busy = true;
                    self.gate_problem = None;
                    vec![Effect::LoginStart(email.clone())]
                }
                _ => Vec::new(),
            },
            Action::OtherAddress => {
                self.busy = false;
                self.gate_problem = None;
                self.gate = Some(Gate::Email);
                Vec::new()
            }
            Action::CheckCode(code) => {
                let code = code.trim().to_owned();
                let whole = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
                match &self.gate {
                    Some(Gate::Code { email, .. }) if whole && !self.busy => {
                        self.busy = true;
                        self.gate_problem = None;
                        vec![Effect::LoginFinish {
                            email: email.clone(),
                            code,
                        }]
                    }
                    _ => Vec::new(),
                }
            }
            Action::PasskeySignIn => {
                if self.busy {
                    return Vec::new();
                }
                self.busy = true;
                self.gate_problem = None;
                vec![Effect::LoginPasskey]
            }
            Action::OpenPasskey => match &self.gate {
                Some(Gate::Passkey { ticket, create, .. }) => {
                    let (ticket, create) = (ticket.clone(), *create);
                    self.wait_for_passkey(ticket, create)
                }
                _ => Vec::new(),
            },
            Action::CancelPasskey => {
                self.poll += 1;
                if let Some(Gate::Passkey { waiting, .. }) = &mut self.gate {
                    *waiting = false;
                }
                Vec::new()
            }
            Action::Link(n) => match &self.gate {
                Some(Gate::Identity(found))
                    if !self.busy
                        && found
                            .get(n)
                            .is_some_and(|identity| identity.source == OWN_SOURCE) =>
                {
                    self.busy = true;
                    self.gate_problem = None;
                    vec![Effect::Link]
                }
                _ => Vec::new(),
            },
            Action::LinkAll => {
                // Flexspeak's own is the only identity found so far, so linking all links it.
                let own = match &self.gate {
                    Some(Gate::Identity(found)) => found
                        .iter()
                        .position(|identity| identity.source == OWN_SOURCE && !identity.linked),
                    _ => None,
                };
                own.map_or_else(Vec::new, |n| self.act(Action::Link(n)))
            }
            Action::Enter => {
                if matches!(self.gate, Some(Gate::Identity(_))) {
                    self.gate = None;
                    self.busy = false;
                    self.gate_problem = None;
                }
                Vec::new()
            }
            Action::SignOut => {
                let session = self.session.clone();
                self.leave();
                self.gate = Some(Gate::Email);
                let mut effects = vec![Effect::Keep(None)];
                effects.extend(session.map(Effect::SignOut));
                effects
            }
        }
    }

    fn wait_for_passkey(&mut self, ticket: String, create: bool) -> Vec<Effect> {
        self.poll += 1;
        self.busy = false;
        self.gate_problem = None;
        self.gate = Some(Gate::Passkey {
            ticket: ticket.clone(),
            create,
            waiting: true,
        });
        let page = api::passkey_page(self.server.as_deref().unwrap_or_default(), &ticket);
        vec![
            Effect::Browse(page),
            Effect::Poll {
                ticket,
                generation: self.poll,
            },
        ]
    }

    /// Signed in: keep the session, offer the identity, and fetch the collection, sending the
    /// cans kept on this computer first if they haven't been.
    fn signed_in(&mut self, session: String, me: api::Me) -> Vec<Effect> {
        self.poll += 1;
        self.busy = false;
        self.gate_problem = None;
        self.session = Some(session.clone());
        self.hidden = me.hidden;
        let import = !me.imported && !self.local.is_empty();
        self.me = Some(me);
        self.gate = Some(Gate::Identity(Vec::new()));
        self.refresh_identities();
        self.cans.clear();
        self.loading = true;
        let mut effects = vec![Effect::Keep(Some(session))];
        if self.own_identity.is_none() {
            effects.push(Effect::FindIdentity);
        }
        effects.push(if import {
            Effect::Import(api::Import {
                cans: self
                    .local
                    .iter()
                    .map(|can| api::ImportedCan {
                        flavour: can.flavour.clone(),
                        at: can.at,
                        rarity: can.rarity,
                        seed: can.seed,
                    })
                    .collect(),
            })
        } else {
            Effect::Cans
        });
        effects
    }

    /// Flexspeak's own identity, looked up for the Identity step.
    pub fn found(&mut self, identity: Option<FoundIdentity>) {
        self.own_identity = identity;
        self.refresh_identities();
    }

    /// The Identity step's list, each marked linked if the account has its uid.
    fn refresh_identities(&mut self) {
        let Some(Gate::Identity(found)) = &mut self.gate else {
            return;
        };
        *found = self.own_identity.iter().cloned().collect();
        let linked = self.me.as_ref().map_or(&[][..], |me| &me.identities[..]);
        for identity in found {
            identity.linked = linked.iter().any(|link| link.uid == identity.uid);
        }
    }

    /// Records a can of this flavour on the server. Taps wait while a card opens or another
    /// can is on its way, so a double tap adds one.
    pub fn add_online(&mut self, flavour: usize) -> Vec<Effect> {
        let Some(flavour) = FLAVOURS.get(flavour) else {
            return Vec::new();
        };
        if self.session.is_none() || self.adding || self.loading || self.reveal.is_some() {
            return Vec::new();
        }
        self.adding = true;
        vec![Effect::Add(api::AddCan {
            flavour: flavour.id.to_owned(),
            key: can_key(),
        })]
    }

    pub fn remove_online(&mut self, identity: &str) -> Vec<Effect> {
        match self.cans.iter().find(|can| can.identity() == identity) {
            Some(can) if self.session.is_some() => vec![Effect::Remove(can.id)],
            _ => Vec::new(),
        }
    }

    /// Shows a part of the page, fetching what it needs from the server.
    pub fn show(&mut self, section: Section) -> Vec<Effect> {
        self.section = section;
        if self.session.is_none() {
            return Vec::new();
        }
        match section {
            Section::Leaderboard => {
                self.people_loading = true;
                self.people_problem = None;
                vec![Effect::Leaderboard(self.ranking)]
            }
            Section::Person(user) => {
                let name = self
                    .people
                    .iter()
                    .find(|person| person.user == user)
                    .map_or_else(String::new, |person| person.name.clone());
                self.visit = Some(Visit {
                    user,
                    name,
                    cans: None,
                    problem: None,
                });
                vec![Effect::Person(user)]
            }
            _ => Vec::new(),
        }
    }

    pub fn rank(&mut self, ranking: Ranking) -> Vec<Effect> {
        self.ranking = ranking;
        if self.session.is_none() {
            return Vec::new();
        }
        self.people_loading = true;
        self.people_problem = None;
        vec![Effect::Leaderboard(ranking)]
    }

    /// Hide Me or Show Me. It shows at once, and goes back if the server refuses.
    pub fn hide(&mut self, hidden: bool) -> Vec<Effect> {
        if self.session.is_none() {
            return Vec::new();
        }
        self.hidden = hidden;
        self.people_problem = None;
        vec![Effect::Hide(hidden)]
    }

    /// What an answer changes, and what to ask next.
    pub fn answer(&mut self, answer: Answer) -> Vec<Effect> {
        if !self.online() {
            return Vec::new();
        }
        // A data answer with no session behind it is from one that has ended.
        let signed = self.session.is_some();
        match answer {
            Answer::LoginStarted(email, result) => {
                if !self.busy {
                    return Vec::new();
                }
                self.busy = false;
                match result {
                    Ok(()) => {
                        let resent = matches!(&self.gate, Some(Gate::Code { email: to, .. }) if *to == email);
                        self.gate = Some(Gate::Code {
                            email,
                            wrong: false,
                            tries_left: None,
                            resent,
                        });
                    }
                    Err(problem) => self.gate_problem = Some(problem.message),
                }
                Vec::new()
            }
            Answer::LoginFinished(result) => {
                if !self.busy {
                    return Vec::new();
                }
                self.busy = false;
                match result {
                    Ok(api::LoginFinished::Passkey { ticket }) => {
                        self.gate = Some(Gate::Passkey {
                            ticket,
                            create: true,
                            waiting: false,
                        });
                        Vec::new()
                    }
                    Ok(api::LoginFinished::SignedIn { session, me }) => self.signed_in(session, me),
                    Err(problem) => {
                        if problem.error == problem::WRONG_CODE
                            && let Some(Gate::Code {
                                wrong, tries_left, ..
                            }) = &mut self.gate
                        {
                            *wrong = true;
                            *tries_left = problem.tries_left;
                        } else {
                            self.gate_problem = Some(problem.message);
                        }
                        Vec::new()
                    }
                }
            }
            Answer::Ticket(result) => {
                if !self.busy {
                    return Vec::new();
                }
                self.busy = false;
                match result {
                    Ok(ticket) => self.wait_for_passkey(ticket.ticket, false),
                    Err(problem) => {
                        self.gate_problem = Some(problem.message);
                        Vec::new()
                    }
                }
            }
            Answer::Polled(generation, result) => {
                let Some(Gate::Passkey {
                    ticket,
                    waiting: true,
                    ..
                }) = &self.gate
                else {
                    return Vec::new();
                };
                if generation != self.poll {
                    return Vec::new();
                }
                let ticket = ticket.clone();
                match result {
                    Ok(api::Polled::Pending) => vec![Effect::Poll { ticket, generation }],
                    Ok(api::Polled::Done { session, me }) => self.signed_in(session, me),
                    // A dropped connection or a restarting server: keep asking until Cancel.
                    Err(problem)
                        if problem.error == crate::monster_api::OFFLINE
                            || problem.error == problem::SERVER =>
                    {
                        vec![Effect::Poll { ticket, generation }]
                    }
                    Err(problem) => {
                        // The ticket ran out: it leads nowhere now, so start again.
                        self.poll += 1;
                        self.gate = Some(Gate::Email);
                        self.gate_problem = Some(problem.message);
                        Vec::new()
                    }
                }
            }
            Answer::Linked(result) => {
                if !self.busy || !signed {
                    return Vec::new();
                }
                self.busy = false;
                match result {
                    Ok(me) => {
                        self.me = Some(me);
                        self.refresh_identities();
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        self.gate_problem = Some(problem.message);
                        Vec::new()
                    }
                }
            }
            Answer::Me(result) => match result {
                Ok(me) if signed => {
                    self.hidden = me.hidden;
                    self.me = Some(me);
                    Vec::new()
                }
                Err(problem) if signed && signed_out(&problem) => self.ended(),
                // Offline, the page carries on with what it has.
                _ => Vec::new(),
            },
            Answer::Cans(result) => {
                if !signed {
                    return Vec::new();
                }
                self.loading = false;
                match result {
                    Ok(cans) => {
                        self.cans = cans.cans.into_iter().map(Can::from).collect();
                        let picked = self.selected.as_deref();
                        if !self
                            .cans
                            .iter()
                            .any(|can| Some(can.identity().as_str()) == picked)
                        {
                            self.selected = None;
                        }
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        self.notice = Some(Notice::Failed(problem.message));
                        Vec::new()
                    }
                }
            }
            Answer::Added(result) => {
                if !signed {
                    return Vec::new();
                }
                self.adding = false;
                match result {
                    Ok(can) => {
                        let can = Can::from(can);
                        let rarity = can.rarity;
                        self.selected = Some(can.identity());
                        // Sent again after a lost answer, the server returns the same can.
                        if !self.cans.iter().any(|old| old.id == can.id) {
                            self.cans.push(can);
                        }
                        self.notice = Some(Notice::Added(rarity));
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        self.notice = Some(Notice::Failed(problem.message));
                        Vec::new()
                    }
                }
            }
            Answer::Removed(id, result) => {
                if !signed {
                    return Vec::new();
                }
                match result {
                    // Already gone counts as gone.
                    Ok(()) => {}
                    Err(problem) if problem.error == problem::NOT_FOUND => {}
                    Err(problem) if signed_out(&problem) => return self.ended(),
                    Err(problem) => {
                        self.notice = Some(Notice::Failed(problem.message));
                        return Vec::new();
                    }
                }
                self.cans.retain(|can| can.id != id);
                self.selected = None;
                self.notice = Some(Notice::Removed);
                Vec::new()
            }
            Answer::Imported(result) => {
                if !signed {
                    return Vec::new();
                }
                match result {
                    // Taken: this account has its cans from a computer already.
                    Ok(_) => self.imported(),
                    Err(problem) if problem.error == problem::TAKEN => self.imported(),
                    Err(problem) if signed_out(&problem) => return self.ended(),
                    // Offered again at the next sign-in.
                    Err(problem) => self.notice = Some(Notice::Failed(problem.message)),
                }
                vec![Effect::Cans]
            }
            Answer::Leaderboard(ranking, result) => {
                if !signed || ranking != self.ranking {
                    return Vec::new();
                }
                self.people_loading = false;
                match result {
                    Ok(board) => {
                        self.people = board.rows.into_iter().map(Person::from).collect();
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        self.people_problem = Some(problem.message);
                        Vec::new()
                    }
                }
            }
            Answer::Person(user, result) => {
                let Some(visit) = self.visit.as_mut().filter(|visit| visit.user == user) else {
                    return Vec::new();
                };
                match result {
                    Ok(collection) => {
                        visit.name = collection.name;
                        visit.cans = Some(collection.cans.into_iter().map(Can::from).collect());
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        visit.problem = Some(if problem.error == problem::NOT_FOUND {
                            "This board is hidden now, or no longer on the leaderboard.".into()
                        } else {
                            problem.message
                        });
                        Vec::new()
                    }
                }
            }
            Answer::Hidden(hidden, result) => {
                if !signed {
                    return Vec::new();
                }
                match result {
                    Ok(me) => {
                        self.hidden = me.hidden;
                        self.me = Some(me);
                        if self.section == Section::Leaderboard {
                            self.people_loading = true;
                            return vec![Effect::Leaderboard(self.ranking)];
                        }
                        Vec::new()
                    }
                    Err(problem) if signed_out(&problem) => self.ended(),
                    Err(problem) => {
                        self.hidden = !hidden;
                        self.people_problem = Some(problem.message);
                        Vec::new()
                    }
                }
            }
        }
    }

    fn imported(&mut self) {
        if let Some(me) = &mut self.me {
            me.imported = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster::{Rarity, Standing};

    const SERVER: &str = "https://collection.example.com";
    const UID: &str = "Zk3vQ0pT8mYw1LhNf6RdXc2Ae9s=";

    fn me() -> api::Me {
        api::Me {
            id: 1,
            name: "sam".into(),
            email: "sam@example.com".into(),
            hidden: false,
            imported: true,
            identities: Vec::new(),
        }
    }

    fn online() -> Tracker {
        Tracker {
            server: Some(SERVER.into()),
            ..Default::default()
        }
    }

    fn signed_in() -> Tracker {
        let mut t = online();
        t.session = Some("s1".into());
        t.session_read = true;
        t.me = Some(me());
        t
    }

    fn problem(error: &str) -> Problem {
        Problem {
            error: error.into(),
            message: format!("{error} happened"),
            tries_left: None,
        }
    }

    fn can(id: i64, flavour: &str, rarity: Rarity) -> api::Can {
        api::Can {
            id,
            flavour: flavour.into(),
            at: 100 + id,
            rarity,
            seed: 7,
            legacy: false,
        }
    }

    fn own() -> FoundIdentity {
        FoundIdentity {
            source: OWN_SOURCE,
            name: "Sam".into(),
            uid: UID.into(),
            linked: false,
        }
    }

    /// Walks to the Code step for sam@example.com.
    fn at_code(t: &mut Tracker) {
        t.connect(None);
        t.act(Action::SendCode("sam@example.com".into()));
        t.answer(Answer::LoginStarted("sam@example.com".into(), Ok(())));
    }

    #[test]
    fn without_a_server_nothing_is_asked() {
        let mut t = Tracker::default();
        assert!(t.connect(Some("old".into())).is_empty());
        assert_eq!(t.gate, None);
        assert!(t.act(Action::SendCode("sam@example.com".into())).is_empty());
        assert!(t.show(Section::Leaderboard).is_empty());
    }

    #[test]
    fn a_kept_session_opens_the_page_and_loads_the_collection() {
        let mut t = online();
        assert_eq!(t.connect(Some("s1".into())), [Effect::Me, Effect::Cans]);
        assert_eq!(t.gate, None);
        assert!(t.loading);
        t.answer(Answer::Cans(Ok(api::Cans {
            cans: vec![can(1, "original", Rarity::Rare)],
        })));
        assert!(!t.loading);
        assert_eq!(t.cans[0].rarity, Rarity::Rare);
        // Without one, signing in.
        let mut t = online();
        assert!(t.connect(None).is_empty());
        assert_eq!(t.gate, Some(Gate::Email));
    }

    #[test]
    fn a_code_goes_only_to_an_address_and_then_waits_for_the_answer() {
        let mut t = online();
        t.connect(None);
        assert!(t.act(Action::SendCode("sam".into())).is_empty());
        assert_eq!(
            t.act(Action::SendCode(" sam@example.com ".into())),
            [Effect::LoginStart("sam@example.com".into())]
        );
        assert!(t.busy);
        assert!(
            t.act(Action::SendCode("sam@example.com".into())).is_empty(),
            "one at a time"
        );
        t.answer(Answer::LoginStarted("sam@example.com".into(), Ok(())));
        assert_eq!(
            t.gate,
            Some(Gate::Code {
                email: "sam@example.com".into(),
                wrong: false,
                tries_left: None,
                resent: false
            })
        );
    }

    #[test]
    fn a_wrong_code_says_how_many_tries_are_left() {
        let mut t = online();
        at_code(&mut t);
        assert!(t.act(Action::CheckCode("12345".into())).is_empty());
        assert_eq!(
            t.act(Action::CheckCode("123456".into())),
            [Effect::LoginFinish {
                email: "sam@example.com".into(),
                code: "123456".into()
            }]
        );
        t.answer(Answer::LoginFinished(Err(Problem {
            tries_left: Some(3),
            ..problem(problem::WRONG_CODE)
        })));
        assert!(matches!(
            t.gate,
            Some(Gate::Code {
                wrong: true,
                tries_left: Some(3),
                ..
            })
        ));
        assert!(!t.busy);
        assert_eq!(t.session, None);
    }

    #[test]
    fn send_again_and_another_address() {
        let mut t = online();
        at_code(&mut t);
        assert_eq!(
            t.act(Action::SendAgain),
            [Effect::LoginStart("sam@example.com".into())]
        );
        t.answer(Answer::LoginStarted("sam@example.com".into(), Ok(())));
        assert!(matches!(t.gate, Some(Gate::Code { resent: true, .. })));
        t.act(Action::OtherAddress);
        assert_eq!(t.gate, Some(Gate::Email));
        // A late answer to a request left behind changes nothing.
        t.answer(Answer::LoginStarted("sam@example.com".into(), Ok(())));
        assert_eq!(t.gate, Some(Gate::Email));
    }

    #[test]
    fn a_right_code_without_passkeys_signs_in_and_offers_the_identity() {
        let mut t = online();
        t.found(Some(own()));
        at_code(&mut t);
        t.act(Action::CheckCode("123456".into()));
        let effects = t.answer(Answer::LoginFinished(Ok(api::LoginFinished::SignedIn {
            session: "s1".into(),
            me: me(),
        })));
        assert_eq!(effects, [Effect::Keep(Some("s1".into())), Effect::Cans]);
        assert_eq!(t.session.as_deref(), Some("s1"));
        assert_eq!(t.gate, Some(Gate::Identity(vec![own()])));
        assert_eq!(t.act(Action::Link(0)), [Effect::Link]);
        let mut linked = me();
        linked.identities.push(api::LinkedIdentity {
            uid: UID.into(),
            source: OWN_SOURCE.into(),
            linked_at: 5,
        });
        t.answer(Answer::Linked(Ok(linked)));
        let Some(Gate::Identity(found)) = &t.gate else {
            panic!("{:?}", t.gate)
        };
        assert!(found[0].linked);
        t.act(Action::Enter);
        assert_eq!(t.gate, None);
    }

    #[test]
    fn the_identity_is_looked_up_when_its_step_comes() {
        let mut t = online();
        at_code(&mut t);
        t.act(Action::CheckCode("123456".into()));
        let effects = t.answer(Answer::LoginFinished(Ok(api::LoginFinished::SignedIn {
            session: "s1".into(),
            me: me(),
        })));
        assert!(effects.contains(&Effect::FindIdentity));
        assert_eq!(t.gate, Some(Gate::Identity(Vec::new())));
        t.found(Some(own()));
        assert_eq!(t.gate, Some(Gate::Identity(vec![own()])));
        // Skip stays possible.
        t.act(Action::Enter);
        assert_eq!(t.gate, None);
    }

    #[test]
    fn a_passkey_is_waited_for_until_the_page_is_done() {
        let mut t = online();
        at_code(&mut t);
        t.act(Action::CheckCode("123456".into()));
        t.answer(Answer::LoginFinished(Ok(api::LoginFinished::Passkey {
            ticket: "t1".into(),
        })));
        assert!(matches!(
            t.gate,
            Some(Gate::Passkey {
                create: true,
                waiting: false,
                ..
            })
        ));
        let effects = t.act(Action::OpenPasskey);
        let Effect::Poll { generation, .. } = effects[1].clone() else {
            panic!("{effects:?}")
        };
        assert_eq!(
            effects,
            [
                Effect::Browse(format!("{SERVER}/passkey#ticket=t1")),
                Effect::Poll {
                    ticket: "t1".into(),
                    generation
                }
            ]
        );
        assert_eq!(
            t.answer(Answer::Polled(generation, Ok(api::Polled::Pending))),
            [Effect::Poll {
                ticket: "t1".into(),
                generation
            }]
        );
        let effects = t.answer(Answer::Polled(
            generation,
            Ok(api::Polled::Done {
                session: "s2".into(),
                me: me(),
            }),
        ));
        assert_eq!(effects[0], Effect::Keep(Some("s2".into())));
        assert!(matches!(t.gate, Some(Gate::Identity(_))));
    }

    #[test]
    fn cancel_stops_the_polling() {
        let mut t = online();
        t.connect(None);
        assert_eq!(t.act(Action::PasskeySignIn), [Effect::LoginPasskey]);
        let effects = t.answer(Answer::Ticket(Ok(api::Ticket {
            ticket: "t9".into(),
        })));
        assert!(
            matches!(effects[0], Effect::Browse(_)),
            "opens straight away"
        );
        let Effect::Poll { generation, .. } = effects[1].clone() else {
            panic!()
        };
        assert!(matches!(
            t.gate,
            Some(Gate::Passkey {
                create: false,
                waiting: true,
                ..
            })
        ));
        t.act(Action::CancelPasskey);
        assert!(
            t.answer(Answer::Polled(generation, Ok(api::Polled::Pending)))
                .is_empty()
        );
        assert!(
            t.answer(Answer::Polled(
                generation,
                Ok(api::Polled::Done {
                    session: "late".into(),
                    me: me()
                })
            ))
            .is_empty()
        );
        assert_eq!(t.session, None);
    }

    #[test]
    fn a_ticket_that_ran_out_starts_again() {
        let mut t = online();
        t.connect(None);
        t.act(Action::PasskeySignIn);
        let effects = t.answer(Answer::Ticket(Ok(api::Ticket { ticket: "t".into() })));
        let Effect::Poll { generation, .. } = effects[1].clone() else {
            panic!()
        };
        assert!(
            !t.answer(Answer::Polled(
                generation,
                Err(problem(crate::monster_api::OFFLINE))
            ))
            .is_empty(),
            "offline keeps asking"
        );
        t.answer(Answer::Polled(generation, Err(problem(problem::EXPIRED))));
        assert_eq!(t.gate, Some(Gate::Email));
        assert!(t.gate_problem.is_some());
    }

    #[test]
    fn a_signed_out_answer_mid_session_returns_to_sign_in() {
        let mut t = signed_in();
        t.cans = vec![can(1, "original", Rarity::Common).into()];
        t.section = Section::Leaderboard;
        let effects = t.answer(Answer::Leaderboard(
            Ranking::Collection,
            Err(problem(problem::SIGNED_OUT)),
        ));
        assert_eq!(effects, [Effect::Keep(None)]);
        assert_eq!(t.session, None);
        assert_eq!(t.gate, Some(Gate::Email));
        assert!(t.cans.is_empty());
        assert_eq!(t.section, Section::Overview);
        // Answers still on their way find nobody to change.
        t.answer(Answer::Cans(Ok(api::Cans {
            cans: vec![can(1, "original", Rarity::Common)],
        })));
        assert!(t.cans.is_empty());
    }

    #[test]
    fn a_kept_session_the_server_ended_asks_to_sign_in() {
        let mut t = online();
        t.connect(Some("old".into()));
        assert_eq!(
            t.answer(Answer::Me(Err(problem(problem::SIGNED_OUT)))),
            [Effect::Keep(None)]
        );
        assert_eq!(t.gate, Some(Gate::Email));
        assert!(t.gate_problem.is_some());
        // Offline, the page carries on.
        let mut t = online();
        t.connect(Some("s1".into()));
        assert!(
            t.answer(Answer::Me(Err(problem(crate::monster_api::OFFLINE))))
                .is_empty()
        );
        assert_eq!(t.session.as_deref(), Some("s1"));
        assert_eq!(t.gate, None);
    }

    #[test]
    fn a_can_is_added_when_the_server_answers_with_its_card() {
        let mut t = signed_in();
        let (index, _) = crate::monster::flavour_by_id("khaotic").unwrap();
        let effects = t.add_online(index);
        let [Effect::Add(add)] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(add.flavour, "khaotic");
        assert_eq!(add.key.len(), 32);
        assert!(t.add_online(index).is_empty(), "a request is out");
        assert!(t.cans.is_empty(), "nothing until the answer");
        t.answer(Answer::Added(Ok(can(4, "khaotic", Rarity::Mythic))));
        assert_eq!(t.cans.len(), 1);
        assert_eq!(t.notice, Some(Notice::Added(Rarity::Mythic)));
        assert_eq!(t.selected.as_deref(), Some("can-4"));
        // The same can again, after a lost answer, isn't a second can.
        t.adding = true;
        t.answer(Answer::Added(Ok(can(4, "khaotic", Rarity::Mythic))));
        assert_eq!(t.cans.len(), 1);
        // Another key per tap.
        let Effect::Add(next) = &t.add_online(index)[0] else {
            panic!()
        };
        assert_ne!(next.key, add.key);
    }

    #[test]
    fn a_can_the_server_refuses_isnt_counted() {
        let mut t = signed_in();
        t.add_online(0);
        t.answer(Answer::Added(Err(problem(problem::SLOW_DOWN))));
        assert!(t.cans.is_empty());
        assert!(!t.adding);
        assert_eq!(t.notice, Some(Notice::Failed("slow_down happened".into())));
        // No taps while a card opens.
        t.reveal = Some(crate::monster::Reveal {
            id: 1,
            identity: "can-1".into(),
            rarity: Rarity::Common,
            step: 0,
        });
        assert!(t.add_online(0).is_empty());
    }

    #[test]
    fn the_bin_removes_a_can_once_the_server_has() {
        let mut t = signed_in();
        t.cans = vec![can(3, "original", Rarity::Common).into()];
        assert_eq!(t.remove_online("can-3"), [Effect::Remove(3)]);
        assert_eq!(t.cans.len(), 1);
        t.answer(Answer::Removed(3, Err(problem(problem::SERVER))));
        assert_eq!(t.cans.len(), 1);
        t.answer(Answer::Removed(3, Ok(())));
        assert!(t.cans.is_empty());
        assert_eq!(t.notice, Some(Notice::Removed));
    }

    #[test]
    fn the_cans_on_this_computer_are_sent_once() {
        let mut t = online();
        t.local = vec![Can {
            id: 1,
            flavour: "ultra-gold".into(),
            at: 50,
            rarity: Rarity::Legendary,
            seed: 3,
        }];
        let mut first = me();
        first.imported = false;
        at_code(&mut t);
        t.act(Action::CheckCode("123456".into()));
        let effects = t.answer(Answer::LoginFinished(Ok(api::LoginFinished::SignedIn {
            session: "s1".into(),
            me: first,
        })));
        let Some(Effect::Import(import)) = effects.last() else {
            panic!("{effects:?}")
        };
        assert_eq!(
            import.cans,
            [api::ImportedCan {
                flavour: "ultra-gold".into(),
                at: 50,
                rarity: Rarity::Legendary,
                seed: 3
            }]
        );
        assert_eq!(
            t.answer(Answer::Imported(Ok(api::Imported { imported: 1 }))),
            [Effect::Cans]
        );
        assert!(t.me.as_ref().unwrap().imported);
        // Signing in again on an account that has them sends nothing.
        t.act(Action::SignOut);
        at_code(&mut t);
        t.act(Action::CheckCode("123456".into()));
        let effects = t.answer(Answer::LoginFinished(Ok(api::LoginFinished::SignedIn {
            session: "s2".into(),
            me: me(),
        })));
        assert_eq!(effects.last(), Some(&Effect::Cans));
        // Refused as taken (imported from another computer), it still loads the collection.
        let mut t = signed_in();
        assert_eq!(
            t.answer(Answer::Imported(Err(problem(problem::TAKEN)))),
            [Effect::Cans]
        );
    }

    #[test]
    fn the_leaderboard_comes_from_the_server_and_a_board_opens_from_it() {
        let mut t = signed_in();
        assert_eq!(
            t.show(Section::Leaderboard),
            [Effect::Leaderboard(Ranking::Collection)]
        );
        let row = |user: i64, name: &str, cards: usize| api::LeaderboardRow {
            user,
            name: name.into(),
            you: user == 1,
            standing: Standing {
                cards,
                legendary: 0,
                flavours: 1,
            },
        };
        t.answer(Answer::Leaderboard(
            Ranking::Collection,
            Ok(api::Leaderboard {
                rows: vec![row(7, "robin", 9), row(1, "sam", 4)],
            }),
        ));
        assert!(!t.people_loading);
        assert_eq!(t.people[0].name, "robin");
        assert_eq!(t.people[0].standing.cards, 9);
        // An answer for a ranking no longer asked for is dropped.
        assert_eq!(
            t.rank(Ranking::Legendary),
            [Effect::Leaderboard(Ranking::Legendary)]
        );
        t.answer(Answer::Leaderboard(
            Ranking::Collection,
            Ok(api::Leaderboard { rows: Vec::new() }),
        ));
        assert_eq!(t.people.len(), 2);

        assert_eq!(t.show(Section::Person(7)), [Effect::Person(7)]);
        assert_eq!(
            t.visit,
            Some(Visit {
                user: 7,
                name: "robin".into(),
                cans: None,
                problem: None
            })
        );
        t.answer(Answer::Person(
            7,
            Ok(api::Collection {
                user: 7,
                name: "robin".into(),
                cans: vec![can(1, "original", Rarity::Rare)],
            }),
        ));
        assert_eq!(t.visit.as_ref().unwrap().cans.as_ref().unwrap().len(), 1);
        // Hidden in the meantime.
        t.show(Section::Person(8));
        t.answer(Answer::Person(8, Err(problem(problem::NOT_FOUND))));
        assert!(t.visit.as_ref().unwrap().problem.is_some());
    }

    #[test]
    fn hide_shows_at_once_and_goes_back_if_refused() {
        let mut t = signed_in();
        t.section = Section::Leaderboard;
        assert_eq!(t.hide(true), [Effect::Hide(true)]);
        assert!(t.hidden);
        let mut hidden = me();
        hidden.hidden = true;
        assert_eq!(
            t.answer(Answer::Hidden(true, Ok(hidden))),
            [Effect::Leaderboard(Ranking::Collection)],
            "the board without you"
        );
        assert!(t.me.as_ref().unwrap().hidden);
        t.hide(false);
        assert!(!t.hidden);
        t.answer(Answer::Hidden(false, Err(problem(problem::SERVER))));
        assert!(t.hidden, "still hidden");
        assert!(t.people_problem.is_some());
    }

    #[test]
    fn sign_out_forgets_the_session_and_ends_it_on_the_server() {
        let mut t = signed_in();
        t.cans = vec![can(1, "original", Rarity::Common).into()];
        assert_eq!(
            t.act(Action::SignOut),
            [Effect::Keep(None), Effect::SignOut("s1".into())]
        );
        assert_eq!(t.session, None);
        assert_eq!(t.me, None);
        assert!(t.cans.is_empty());
        assert_eq!(t.gate, Some(Gate::Email));
    }

    #[test]
    fn another_server_starts_from_nothing() {
        let mut t = signed_in();
        t.set_server(Some("https://other.example.com".into()));
        assert_eq!(t.session, None);
        assert!(!t.session_read);
        t.set_server(None);
        assert!(!t.online());
        assert_eq!(t.gate, None);
    }

    #[test]
    fn a_server_address_must_be_a_web_address() {
        assert_eq!(server_address("  "), Ok(None));
        assert_eq!(
            server_address(" https://collection.example.com/ "),
            Ok(Some("https://collection.example.com".into()))
        );
        assert!(server_address("collection.example.com").is_err());
        assert!(server_address("https://").is_err());
        assert!(server_address("https://a b").is_err());
    }
}
