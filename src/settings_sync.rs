//! Keeps the settings in [`settings::Synced`] the same on every computer signed in to one
//! collection server account. The server keeps one copy and a version that counts saves;
//! `monster_core::api` describes the rules.
//!
//! As in `monster::online`, nothing here touches the network, the disk or the clock: each call
//! takes the time and the settings as they are, and returns [`Effect`]s for `main` to run.
//! Tests feed in made-up answers.

use std::time::{Duration, Instant};

use crate::monster_api::{OFFLINE, Problem, SettingsSaved, SyncedSettings, problem};
use crate::settings::{self, Synced};

/// How long after a change it is sent, so a burst of changes goes as one save.
pub const SAVE_DELAY: Duration = Duration::from_secs(2);
/// How often the server's copy is checked while signed in.
pub const FETCH_EVERY: Duration = Duration::from_secs(5 * 60);
/// How soon a request that didn't reach the server is tried again.
pub const RETRY: Duration = Duration::from_secs(30);

/// Something for `main` to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Ask the server for its copy.
    Fetch,
    /// Send this computer's settings, based on the version it last saw.
    Save { base: i64, data: serde_json::Value },
    /// Take these settings, as if they had been changed here by hand, and save them.
    Apply(Box<Synced>),
    /// Keep this version in settings.json; 0 forgets it.
    Remember(i64),
    /// The server said the session has ended: the Monster page asks to sign in again.
    SessionEnded,
}

/// What the server answered. Each carries the [`SettingsSync::generation`] the request was
/// sent in, so an answer to a request from before signing out is dropped.
#[derive(Debug, Clone)]
pub enum Answer {
    /// The account's copy; `None` if nothing was saved yet.
    Fetched(u64, Result<Option<SyncedSettings>, Problem>),
    Saved(u64, Result<SettingsSaved, Problem>),
}

/// Which copy to keep when this computer and the account first meet and differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Use My Account's Settings.
    Account,
    /// Keep This Computer's.
    ThisComputer,
}

/// Where it stands, for the Monster page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    SignedOut,
    Syncing,
    /// With when this computer last agreed with the server, if it has since launch.
    UpToDate(Option<Instant>),
    Offline,
    /// The account's copy differs from this computer's, and nothing changes until a choice.
    Choose,
}

/// What the deciding needs from the app each time.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// A collection server session exists.
    pub signed_in: bool,
    /// First-run setup is showing.
    pub onboarding: bool,
    /// The saved settings; `None` before setup has saved any.
    pub local: Option<&'a settings::Settings>,
}

impl Context<'_> {
    fn active(&self) -> bool {
        self.signed_in && !self.onboarding && self.local.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Fetch,
    Save,
}

#[derive(Debug, Default)]
pub struct SettingsSync {
    /// The version this computer last saw, as saved; 0 for none.
    pub seen: i64,
    /// Moves on whenever answers on their way should be dropped.
    generation: u64,
    busy: Option<Request>,
    /// The server answered since launch or signing in; nothing is saved
    /// before, since this computer doesn't know yet what the server has.
    fetched: bool,
    /// When the next check is due; `None` checks at once.
    next_fetch: Option<Instant>,
    /// A change waits to be sent until then.
    save_due: Option<Instant>,
    /// What travelled when last looked at, to notice a change; `None` takes the next look as
    /// the starting point, which is how taking a fetched copy avoids sending it back.
    observed: Option<Synced>,
    /// The server's copy as last seen, so a save keeps the fields only newer versions know.
    server_data: Option<serde_json::Map<String, serde_json::Value>>,
    /// The copy sent in the save that is out, which becomes `server_data` once taken.
    sending: Option<serde_json::Map<String, serde_json::Value>>,
    /// The account's copy, while waiting for [`Choice`].
    pub choice: Option<SyncedSettings>,
    pub offline: bool,
    pub last_synced: Option<Instant>,
}

impl SettingsSync {
    pub fn new(seen: i64) -> Self {
        Self {
            seen,
            ..Self::default()
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn status(&self, signed_in: bool) -> Status {
        if !signed_in {
            Status::SignedOut
        } else if self.choice.is_some() {
            Status::Choose
        } else if self.offline {
            Status::Offline
        } else if self.busy == Some(Request::Save) || !self.fetched {
            Status::Syncing
        } else {
            Status::UpToDate(self.last_synced)
        }
    }

    /// Forgets everything about the current exchange with the server, but not the version.
    fn restart(&mut self) {
        self.generation += 1;
        self.busy = None;
        self.fetched = false;
        self.next_fetch = None;
        self.save_due = None;
        self.observed = None;
        self.server_data = None;
        self.sending = None;
        self.choice = None;
        self.offline = false;
    }

    /// Runs every second or so: notices changes, and sends or checks when one is due.
    pub fn tick(&mut self, now: Instant, ctx: Context) -> Vec<Effect> {
        if !ctx.active() {
            return Vec::new();
        }
        let Some(local) = ctx.local else {
            return Vec::new();
        };
        let current = Synced::from_settings(local);
        match &self.observed {
            None => self.observed = Some(current),
            Some(observed) if *observed != current => {
                self.observed = Some(current);
                self.save_due = Some(now + SAVE_DELAY);
            }
            Some(_) => {}
        }
        if self.busy.is_some() {
            return Vec::new();
        }
        if self.fetched && self.choice.is_none() && self.save_due.is_some_and(|due| now >= due) {
            self.save_due = None;
            return vec![self.save(self.seen, local)];
        }
        if self.next_fetch.is_none_or(|due| now >= due) {
            return vec![self.fetch(now)];
        }
        Vec::new()
    }

    fn fetch(&mut self, now: Instant) -> Effect {
        self.busy = Some(Request::Fetch);
        self.next_fetch = Some(now + FETCH_EVERY);
        Effect::Fetch
    }

    /// Sends `local`'s travelling settings over the server's copy, so fields from newer
    /// versions of the app survive a save from this one.
    fn save(&mut self, base: i64, local: &settings::Settings) -> Effect {
        let mut data = self.server_data.clone().unwrap_or_default();
        if let Ok(serde_json::Value::Object(ours)) =
            serde_json::to_value(Synced::from_settings(local))
        {
            data.extend(ours);
        }
        self.busy = Some(Request::Save);
        self.sending = Some(data.clone());
        Effect::Save {
            base,
            data: serde_json::Value::Object(data),
        }
    }

    /// A session began: check the server's copy straight away.
    pub fn signed_in(&mut self, now: Instant, ctx: Context) -> Vec<Effect> {
        self.restart();
        self.tick(now, ctx)
    }

    /// The session ended: stop, and forget the version so the next account starts by
    /// comparing.
    pub fn signed_out(&mut self) -> Vec<Effect> {
        self.restart();
        if self.seen == 0 {
            return Vec::new();
        }
        self.seen = 0;
        vec![Effect::Remember(0)]
    }

    /// What the server answered.
    pub fn answer(&mut self, now: Instant, answer: Answer, ctx: Context) -> Vec<Effect> {
        let generation = match &answer {
            Answer::Fetched(generation, _) | Answer::Saved(generation, _) => *generation,
        };
        if generation != self.generation || !ctx.active() {
            return Vec::new();
        }
        let Some(local) = ctx.local else {
            return Vec::new();
        };
        self.busy = None;
        match answer {
            Answer::Fetched(_, Ok(copy)) => {
                self.fetched = true;
                self.offline = false;
                self.compare(now, copy, local)
            }
            Answer::Saved(_, Ok(saved)) => {
                self.offline = false;
                self.server_data = self.sending.take();
                self.seen = saved.version;
                self.last_synced = Some(now);
                vec![Effect::Remember(saved.version)]
            }
            Answer::Fetched(_, Err(problem)) | Answer::Saved(_, Err(problem))
                if problem.error == problem::SIGNED_OUT =>
            {
                let mut effects = self.signed_out();
                effects.push(Effect::SessionEnded);
                effects
            }
            // Another computer saved first: its copy wins, and this computer's change is lost.
            Answer::Saved(_, Err(problem)) if problem.error == problem::CONFLICT => {
                self.sending = None;
                self.save_due = None;
                vec![self.fetch(now)]
            }
            Answer::Saved(_, Err(problem)) => {
                self.trouble(&problem);
                self.sending = None;
                self.save_due = Some(now + RETRY);
                Vec::new()
            }
            Answer::Fetched(_, Err(problem)) => {
                self.trouble(&problem);
                self.next_fetch = Some(now + RETRY);
                Vec::new()
            }
        }
    }

    /// A request that failed for any other reason: say so and try again soon. Nothing local
    /// changes.
    fn trouble(&mut self, problem: &Problem) {
        if problem.error != OFFLINE {
            tracing::warn!("Settings sync: {}", problem.message);
        }
        self.offline = true;
    }

    fn compare(
        &mut self,
        now: Instant,
        copy: Option<SyncedSettings>,
        local: &settings::Settings,
    ) -> Vec<Effect> {
        let Some(copy) = copy else {
            // Nothing on the server, or it lost what it had: this computer's settings start it.
            self.server_data = None;
            self.choice = None;
            return vec![self.save(0, local)];
        };
        self.server_data = match &copy.data {
            serde_json::Value::Object(map) => Some(map.clone()),
            _ => None,
        };
        let synced = Synced::read(&copy.data);
        if self.seen == 0 {
            // First contact: take a copy that changes nothing, otherwise wait for a choice.
            if synced.matches(local) {
                self.choice = None;
                return self.agree(now, copy.version);
            }
            self.choice = Some(copy);
            return Vec::new();
        }
        if copy.version == self.seen {
            // The server has what this computer last saw. If the settings here differ, a
            // change made while offline, or just before the app last closed, never went out.
            if !synced.matches(local) {
                self.save_due = None;
                return vec![self.save(copy.version, local)];
            }
            self.last_synced = Some(now);
            return Vec::new();
        }
        // Newer, or a server that went back: either way its copy is the account's now.
        self.take(now, synced, copy.version)
    }

    fn agree(&mut self, now: Instant, version: i64) -> Vec<Effect> {
        self.seen = version;
        self.last_synced = Some(now);
        vec![Effect::Remember(version)]
    }

    fn take(&mut self, now: Instant, synced: Synced, version: i64) -> Vec<Effect> {
        self.save_due = None;
        self.observed = None;
        let mut effects = vec![Effect::Apply(Box::new(synced))];
        effects.extend(self.agree(now, version));
        effects
    }

    /// One of the two buttons shown while waiting for a choice.
    pub fn choose(&mut self, choice: Choice, now: Instant, ctx: Context) -> Vec<Effect> {
        if !ctx.active() || self.busy.is_some() {
            return Vec::new();
        }
        let (Some(copy), Some(local)) = (self.choice.take(), ctx.local) else {
            return Vec::new();
        };
        match choice {
            Choice::Account => self.take(now, Synced::read(&copy.data), copy.version),
            Choice::ThisComputer => {
                self.save_due = None;
                vec![self.save(copy.version, local)]
            }
        }
    }
}

/// How long ago, as the status line says it: "just now", "2 minutes ago", "1 hour ago".
pub fn ago(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "just now".into(),
        1 => "1 minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "1 hour ago".into(),
        _ => format!("{} hours ago", minutes / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn problem(error: &str) -> Problem {
        Problem {
            error: error.into(),
            message: format!("{error} happened"),
            tries_left: None,
        }
    }

    fn ctx(local: &Settings) -> Context<'_> {
        Context {
            signed_in: true,
            onboarding: false,
            local: Some(local),
        }
    }

    fn copy(version: i64, settings: &Settings) -> SyncedSettings {
        SyncedSettings {
            version,
            updated_at: 1_700_000_000,
            data: serde_json::to_value(Synced::from_settings(settings)).unwrap(),
        }
    }

    fn renamed(nickname: &str) -> Settings {
        Settings {
            nickname: nickname.into(),
            ..Settings::default()
        }
    }

    /// Signed in, and the first check sent.
    fn started(seen: i64, now: Instant, local: &Settings) -> SettingsSync {
        let mut sync = SettingsSync::new(seen);
        assert_eq!(sync.signed_in(now, ctx(local)), [Effect::Fetch]);
        sync
    }

    fn fetched(
        sync: &mut SettingsSync,
        now: Instant,
        copy: Option<SyncedSettings>,
        local: &Settings,
    ) -> Vec<Effect> {
        let generation = sync.generation();
        sync.answer(now, Answer::Fetched(generation, Ok(copy)), ctx(local))
    }

    fn saved(sync: &mut SettingsSync, now: Instant, version: i64, local: &Settings) -> Vec<Effect> {
        let generation = sync.generation();
        let answer = Answer::Saved(
            generation,
            Ok(SettingsSaved {
                version,
                updated_at: 1_700_000_000,
            }),
        );
        sync.answer(now, answer, ctx(local))
    }

    fn save_base(effects: &[Effect]) -> Option<i64> {
        match effects {
            [Effect::Save { base, .. }] => Some(*base),
            _ => None,
        }
    }

    /// Up to date at version 3, as after a first fetch.
    fn in_step(now: Instant, local: &Settings) -> SettingsSync {
        let mut sync = started(3, now, local);
        assert!(fetched(&mut sync, now, Some(copy(3, local)), local).is_empty());
        assert_eq!(sync.status(true), Status::UpToDate(Some(now)));
        sync
    }

    #[test]
    fn first_contact_with_no_copy_uploads_this_computers() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(0, now, &local);
        let effects = fetched(&mut sync, now, None, &local);
        let [Effect::Save { base: 0, data }] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(Synced::read(data), Synced::from_settings(&local));
        assert_eq!(sync.status(true), Status::Syncing);
        assert_eq!(saved(&mut sync, now, 1, &local), [Effect::Remember(1)]);
        assert_eq!(sync.seen, 1);
        assert_eq!(sync.status(true), Status::UpToDate(Some(now)));
    }

    #[test]
    fn first_contact_with_the_same_copy_just_remembers_its_version() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(0, now, &local);
        assert_eq!(
            fetched(&mut sync, now, Some(copy(4, &local)), &local),
            [Effect::Remember(4)]
        );
        assert_eq!(sync.seen, 4);
        assert_eq!(sync.choice, None);
    }

    #[test]
    fn first_contact_with_a_differing_copy_waits_and_uses_the_accounts_when_chosen() {
        let now = Instant::now();
        let local = renamed("Sam");
        let theirs = renamed("Robin");
        let mut sync = started(0, now, &local);
        assert!(fetched(&mut sync, now, Some(copy(6, &theirs)), &local).is_empty());
        assert_eq!(sync.status(true), Status::Choose);
        // Nothing changes in either place until the choice, whatever happens meanwhile.
        let later = now + FETCH_EVERY / 2;
        let changed = renamed("Sam B");
        assert!(sync.tick(later, ctx(&changed)).is_empty());
        assert!(sync.tick(later + SAVE_DELAY, ctx(&changed)).is_empty());
        assert_eq!(sync.seen, 0);

        let effects = sync.choose(Choice::Account, later, ctx(&changed));
        let [Effect::Apply(synced), Effect::Remember(6)] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(synced.nickname.as_deref(), Some("Robin"));
        assert_eq!(sync.seen, 6);
        assert_eq!(sync.status(true), Status::UpToDate(Some(later)));
    }

    #[test]
    fn keeping_this_computers_replaces_the_accounts_copy() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(0, now, &local);
        fetched(&mut sync, now, Some(copy(6, &renamed("Robin"))), &local);
        let effects = sync.choose(Choice::ThisComputer, now, ctx(&local));
        let [Effect::Save { base: 6, data }] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(Synced::read(data).nickname.as_deref(), Some("Sam"));
        assert_eq!(saved(&mut sync, now, 7, &local), [Effect::Remember(7)]);
        assert_eq!(sync.choice, None);
    }

    #[test]
    fn a_newer_version_is_applied_and_applying_it_sends_nothing_back() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        let later = now + FETCH_EVERY;
        assert_eq!(sync.tick(later, ctx(&local)), [Effect::Fetch]);
        let effects = fetched(&mut sync, later, Some(copy(5, &renamed("Robin"))), &local);
        let [Effect::Apply(synced), Effect::Remember(5)] = &effects[..] else {
            panic!("{effects:?}")
        };
        // `main` takes it, which changes what travels here.
        let mut taken = local.clone();
        synced.as_ref().clone().apply(&mut taken);
        assert_eq!(taken.nickname, "Robin");
        for second in 1..10 {
            let at = later + Duration::from_secs(second);
            assert!(sync.tick(at, ctx(&taken)).is_empty(), "no save back");
        }
        assert_eq!(sync.seen, 5);
    }

    #[test]
    fn a_change_is_saved_after_the_delay_and_a_burst_goes_as_one() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        let one = renamed("Sa");
        let two = renamed("Sam B");
        let second = Duration::from_secs(1);
        assert!(sync.tick(now + second, ctx(&one)).is_empty());
        assert!(sync.tick(now + 2 * second, ctx(&two)).is_empty());
        assert!(
            sync.tick(now + 3 * second, ctx(&two)).is_empty(),
            "too soon"
        );
        let effects = sync.tick(now + 4 * second, ctx(&two));
        let [Effect::Save { base: 3, data }] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(Synced::read(data).nickname.as_deref(), Some("Sam B"));
        assert!(
            sync.tick(now + 5 * second, ctx(&two)).is_empty(),
            "one at a time"
        );
        assert_eq!(
            saved(&mut sync, now + 5 * second, 4, &two),
            [Effect::Remember(4)]
        );
        assert!(sync.tick(now + 9 * second, ctx(&two)).is_empty());
    }

    #[test]
    fn a_local_only_change_is_not_sent() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        let moved = Settings {
            playback_volume_percent: 40,
            input_device_id: Some("input-id".into()),
            sidebar_width: Some(300),
            ..local.clone()
        };
        for second in 1..10 {
            let at = now + Duration::from_secs(second);
            assert!(sync.tick(at, ctx(&moved)).is_empty());
        }
    }

    #[test]
    fn a_save_another_computer_beat_takes_its_copy() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        let mine = renamed("Sam B");
        sync.tick(now, ctx(&mine));
        assert_eq!(save_base(&sync.tick(now + SAVE_DELAY, ctx(&mine))), Some(3));
        let generation = sync.generation();
        assert_eq!(
            sync.answer(
                now,
                Answer::Saved(generation, Err(problem(problem::CONFLICT))),
                ctx(&mine)
            ),
            [Effect::Fetch]
        );
        let effects = fetched(&mut sync, now, Some(copy(4, &renamed("Robin"))), &mine);
        assert!(matches!(
            &effects[..],
            [Effect::Apply(synced), Effect::Remember(4)] if synced.nickname.as_deref() == Some("Robin")
        ));
    }

    #[test]
    fn a_signed_out_answer_stops_and_forgets_the_version() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(3, now, &local);
        let generation = sync.generation();
        assert_eq!(
            sync.answer(
                now,
                Answer::Fetched(generation, Err(problem(problem::SIGNED_OUT))),
                ctx(&local)
            ),
            [Effect::Remember(0), Effect::SessionEnded]
        );
        assert_eq!(sync.seen, 0);
        // The app has no session now, so nothing more is asked.
        let gone = Context {
            signed_in: false,
            ..ctx(&local)
        };
        assert!(sync.tick(now + FETCH_EVERY, gone).is_empty());
        assert_eq!(sync.status(false), Status::SignedOut);
    }

    #[test]
    fn a_change_that_never_went_out_is_sent_at_the_next_look() {
        let now = Instant::now();
        let before = Settings::default();
        let after = renamed("Kim");
        // The server still has version 4, from before the nickname changed here.
        let mut sync = started(4, now, &after);
        let effects = fetched(&mut sync, now, Some(copy(4, &before)), &after);
        assert_eq!(save_base(&effects), Some(4));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Apply(_))));
    }

    #[test]
    fn offline_leaves_everything_alone_and_tries_again_soon() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        let mine = renamed("Sam B");
        sync.tick(now, ctx(&mine));
        assert!(save_base(&sync.tick(now + SAVE_DELAY, ctx(&mine))).is_some());
        let generation = sync.generation();
        assert!(
            sync.answer(
                now + SAVE_DELAY,
                Answer::Saved(generation, Err(problem(OFFLINE))),
                ctx(&mine)
            )
            .is_empty(),
            "nothing applied, nothing forgotten"
        );
        assert_eq!(sync.seen, 3);
        assert_eq!(sync.status(true), Status::Offline);
        assert!(
            sync.tick(now + SAVE_DELAY + RETRY / 2, ctx(&mine))
                .is_empty()
        );
        let effects = sync.tick(now + SAVE_DELAY + RETRY, ctx(&mine));
        assert_eq!(save_base(&effects), Some(3), "{effects:?}");
        assert_eq!(saved(&mut sync, now, 4, &mine), [Effect::Remember(4)]);
        assert_eq!(sync.status(true), Status::UpToDate(Some(now)));

        // A check that fails is tried again too, and changes nothing.
        let mut sync = started(3, now, &local);
        let generation = sync.generation();
        assert!(
            sync.answer(
                now,
                Answer::Fetched(generation, Err(problem(problem::SERVER))),
                ctx(&local)
            )
            .is_empty()
        );
        assert_eq!(sync.status(true), Status::Offline);
        assert_eq!(sync.tick(now + RETRY, ctx(&local)), [Effect::Fetch]);
    }

    #[test]
    fn nothing_is_sent_or_checked_before_the_first_answer() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(3, now, &local);
        let mine = renamed("Sam B");
        sync.tick(now, ctx(&mine));
        assert!(sync.tick(now + SAVE_DELAY, ctx(&mine)).is_empty());
        // The change waits for the check, then goes based on what it found, and only once.
        let effects = fetched(&mut sync, now, Some(copy(3, &local)), &mine);
        assert_eq!(save_base(&effects), Some(3));
        assert!(sync.tick(now + SAVE_DELAY, ctx(&mine)).is_empty());
    }

    #[test]
    fn not_during_first_run_setup() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = SettingsSync::new(3);
        let setup = Context {
            onboarding: true,
            ..ctx(&local)
        };
        assert!(sync.signed_in(now, setup).is_empty());
        assert!(sync.tick(now + FETCH_EVERY, setup).is_empty());
        let unsaved = Context {
            local: None,
            ..ctx(&local)
        };
        assert!(sync.tick(now + FETCH_EVERY, unsaved).is_empty());
        assert_eq!(sync.tick(now + FETCH_EVERY, ctx(&local)), [Effect::Fetch]);
    }

    #[test]
    fn signing_out_forgets_the_version_and_drops_answers_on_their_way() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(3, now, &local);
        let generation = sync.generation();
        assert_eq!(sync.signed_out(), [Effect::Remember(0)]);
        assert!(sync.signed_out().is_empty(), "once");
        assert_eq!(sync.seen, 0);
        // Signed in again straight away, the old answer doesn't count for the new session.
        assert_eq!(sync.signed_in(now, ctx(&local)), [Effect::Fetch]);
        let late = Answer::Fetched(generation, Ok(Some(copy(9, &renamed("Robin")))));
        assert!(sync.answer(now, late, ctx(&local)).is_empty());
        assert_eq!(sync.status(true), Status::Syncing);
    }

    #[test]
    fn checks_every_five_minutes() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = in_step(now, &local);
        assert!(sync.tick(now + FETCH_EVERY / 2, ctx(&local)).is_empty());
        assert_eq!(sync.tick(now + FETCH_EVERY, ctx(&local)), [Effect::Fetch]);
    }

    #[test]
    fn a_save_keeps_fields_only_a_newer_version_knows() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(3, now, &local);
        let mut newer = copy(3, &local);
        newer.data["laser_mode"] = serde_json::json!(true);
        fetched(&mut sync, now, Some(newer), &local);
        let mine = renamed("Sam B");
        sync.tick(now, ctx(&mine));
        let effects = sync.tick(now + SAVE_DELAY, ctx(&mine));
        let [Effect::Save { data, .. }] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(data["laser_mode"], true);
        assert_eq!(data["nickname"], "Sam B");
    }

    #[test]
    fn a_server_that_lost_its_copy_is_given_this_computers_again() {
        let now = Instant::now();
        let local = renamed("Sam");
        let mut sync = started(3, now, &local);
        assert_eq!(save_base(&fetched(&mut sync, now, None, &local)), Some(0));
    }

    #[test]
    fn the_status_line_says_how_long_ago_in_words() {
        assert_eq!(ago(Duration::from_secs(20)), "just now");
        assert_eq!(ago(Duration::from_secs(60)), "1 minute ago");
        assert_eq!(ago(Duration::from_secs(150)), "2 minutes ago");
        assert_eq!(ago(Duration::from_secs(3_700)), "1 hour ago");
        assert_eq!(ago(Duration::from_secs(3 * 3_600)), "3 hours ago");
    }
}
