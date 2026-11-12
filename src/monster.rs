//! The Monster can collection: pick the flavour you just drank, and it becomes a card.
//!
//! Each can's rarity is rolled once, when it's added, and saved with it, so nothing can reroll
//! it later. The roll mixes a random key kept in the database with the flavour, the time and
//! the can's number. The collection lives in `monster.sqlite` beside `settings.json`.
//!
//! With a collection server set (Settings › WIP), the server keeps the collection and rolls the
//! cards instead, and the page needs an account; `online` decides what each answer changes.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration as StdDuration,
};

use anyhow::{Context, Result, bail};
use chrono::{Duration, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub use monster_core::{
    CARDS, FLAVOURS, Flavour, Ranking, Rarity, Standing, api, draw, flavour_by_id,
    flavour_or_other, from_hash,
};
pub use online::{Action, Answer, Effect, OWN_SOURCE, Visit, server_address};

mod online;
/// How many more cards the collection shows at a time. Every card is several shader layers.
pub const PAGE: usize = 48;
/// A standard can, for the board's totals.
const LITRES_PER_CAN: f32 = 0.5;
const CAFFEINE_MG_PER_CAN: u32 = 160;

/// The luck of a can saved by the scanning version, from its barcode and printed codes, so
/// those cards keep the rarity they were opened with.
fn legacy_roll(barcode: &str, code: &str, copy: u32) -> (Rarity, u16) {
    let mut identity = if barcode.is_empty() {
        code.to_owned()
    } else {
        format!("{barcode}-{code}")
    };
    if copy > 0 {
        identity.push_str(&format!("-{}", copy + 1));
    }
    from_hash(&Sha256::digest(format!("flexspeak-monster-v1:{identity}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Can {
    /// Its number in the collection, from 1, and never reused.
    pub id: i64,
    /// A `Flavour::id`.
    pub flavour: String,
    /// When it was drunk, in Unix seconds.
    pub at: i64,
    pub rarity: Rarity,
    /// Varies the card's art between cans of a flavour.
    pub seed: u16,
}

impl From<api::Can> for Can {
    /// A can as the server keeps it. A legacy can, uploaded from a computer, shows like any other.
    fn from(can: api::Can) -> Self {
        Self {
            id: can.id,
            flavour: can.flavour,
            at: can.at,
            rarity: can.rarity,
            seed: can.seed,
        }
    }
}

impl Can {
    /// What tells this can from every other in the collection.
    pub fn identity(&self) -> String {
        format!("can-{}", self.id)
    }

    /// How the card names the can.
    pub fn number(&self) -> String {
        format!("#{}", self.id)
    }

    pub fn rarity(&self) -> Rarity {
        self.rarity
    }

    pub fn seed(&self) -> u16 {
        self.seed
    }

    pub fn flavour(&self) -> &'static Flavour {
        flavour_or_other(&self.flavour)
    }

    /// The flavour's place in `FLAVOURS`, which the shaders read.
    pub fn flavour_index(&self) -> usize {
        flavour_by_id(&self.flavour).map_or(FLAVOURS.len() - 1, |(index, _)| index)
    }
}

pub fn path(settings: &Path) -> PathBuf {
    settings.with_file_name("monster.sqlite")
}

/// 1 was the scanning version, which kept barcodes and printed codes.
const SCHEMA_VERSION: i64 = 2;

fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Can't create the folder for monster.sqlite")?;
    }
    let connection = Connection::open(path).context("Can't open monster.sqlite")?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("monster.sqlite isn't a can collection")?;
    if version > SCHEMA_VERSION {
        bail!("monster.sqlite was made by a newer Flexspeak");
    }
    let transaction = connection.unchecked_transaction()?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS drinks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 flavour TEXT NOT NULL,
                 at INTEGER NOT NULL,
                 rarity INTEGER NOT NULL,
                 seed INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value BLOB NOT NULL
             );",
        )
        .context("monster.sqlite isn't a can collection")?;
    if version == 1 {
        migrate_scanned(&transaction)?;
    }
    transaction.execute_batch("PRAGMA user_version = 2;")?;
    transaction
        .commit()
        .context("monster.sqlite isn't a can collection")?;
    Ok(connection)
}

/// Moves the scanning version's cans across with the rarity they were opened with. Its tables
/// stay behind, renamed, as a backup.
fn migrate_scanned(connection: &Connection) -> Result<()> {
    let old: Vec<(String, String, u32, String, i64)> = connection
        .prepare("SELECT barcode, code, copy, flavour, at FROM cans ORDER BY id")?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    for (barcode, code, copy, flavour, at) in old {
        let (rarity, seed) = legacy_roll(&barcode, &code, copy);
        connection.execute(
            "INSERT INTO drinks (flavour, at, rarity, seed) VALUES (?1, ?2, ?3, ?4)",
            params![flavour, at, rarity.index() as i64, seed],
        )?;
    }
    connection.execute_batch(
        "ALTER TABLE cans RENAME TO scanned_cans;
         ALTER TABLE barcodes RENAME TO scanned_barcodes;",
    )?;
    Ok(())
}

/// The collection's secret roll key, made the first time it's needed.
fn roll_key(connection: &Connection) -> Result<[u8; 32]> {
    let saved: Option<Vec<u8>> = connection
        .query_row("SELECT value FROM meta WHERE key = 'roll_key'", [], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(key) = saved.and_then(|saved| <[u8; 32]>::try_from(saved).ok()) {
        return Ok(key);
    }
    let key = new_key()?;
    connection.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('roll_key', ?1)",
        params![key.to_vec()],
    )?;
    Ok(key)
}

fn new_key() -> Result<[u8; 32]> {
    let mut key = [0; 32];
    getrandom::fill(&mut key).map_err(|error| anyhow::anyhow!("No randomness: {error}"))?;
    Ok(key)
}

/// The cans in `monster.sqlite`, or none if there's no such file.
fn read_local(path: &Path) -> Result<Vec<Can>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_collection(&open_database(path)?)
}

fn read_collection(connection: &Connection) -> Result<Vec<Can>> {
    let mut statement =
        connection.prepare("SELECT id, flavour, at, rarity, seed FROM drinks ORDER BY id")?;
    let cans = statement
        .query_map([], |row| {
            let rarity: i64 = row.get(3)?;
            Ok(Can {
                id: row.get(0)?,
                flavour: row.get(1)?,
                at: row.get(2)?,
                rarity: usize::try_from(rarity)
                    .ok()
                    .and_then(|rarity| Rarity::ALL.get(rarity).copied())
                    .unwrap_or(Rarity::Common),
                seed: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(cans)
}

/// The flavours to offer, most drunk first, then the latest drunk, then in their usual order,
/// with how often each was drunk. A search keeps only the flavours whose name holds it.
pub fn favourites(cans: &[Can], query: &str) -> Vec<(usize, u32)> {
    let mut counts = vec![(0u32, i64::MIN); FLAVOURS.len()];
    for can in cans {
        let entry = &mut counts[can.flavour_index()];
        entry.0 += 1;
        entry.1 = entry.1.max(can.at);
    }
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut order: Vec<usize> = (0..FLAVOURS.len())
        .filter(|&index| {
            let flavour = &FLAVOURS[index];
            let name = format!("{} {}", flavour.name, flavour.line).to_lowercase();
            words.iter().all(|word| name.contains(word))
        })
        .collect();
    // Stable, so flavours never drunk keep the order they're listed in.
    order.sort_by_key(|&index| std::cmp::Reverse(counts[index]));
    order
        .into_iter()
        .map(|index| (index, counts[index].0))
        .collect()
}

/// The part of the page on show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Overview,
    /// Every flavour, found or not.
    Flavours,
    /// One flavour's page, by its place in `FLAVOURS`.
    Flavour(usize),
    /// Everyone who shows on the leaderboard.
    Leaderboard,
    /// Someone's board, by their account's id on the server.
    Person(i64),
}

impl Section {
    /// Where Back goes; `None` leaves the page.
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::Overview => None,
            Self::Flavours => Some(Self::Overview),
            Self::Flavour(_) => Some(Self::Flavours),
            Self::Leaderboard => Some(Self::Overview),
            Self::Person(_) => Some(Self::Leaderboard),
        }
    }
}

/// Someone on the leaderboard, as the server ranks them. Their cans aren't in the list; opening
/// their board fetches them (`Visit`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Person {
    /// Their account's id on the server.
    pub user: i64,
    pub name: String,
    /// The person signed in, among the rest.
    pub you: bool,
    /// Counted by the server from the cards it rolled.
    pub standing: Standing,
}

impl From<api::LeaderboardRow> for Person {
    fn from(row: api::LeaderboardRow) -> Self {
        Self {
            user: row.user,
            name: row.name,
            you: row.you,
            standing: row.standing,
        }
    }
}

/// Where a collection stands on the leaderboard.
pub fn standing(cans: &[Can]) -> Standing {
    Standing::count(cans.iter().map(|can| (can.flavour.as_str(), can.rarity)))
}

/// The people on the leaderboard, best first. The server sends them in this order already;
/// sorting again keeps a list fetched for another ranking right until the new one arrives. Ties
/// keep the order they come in, so a tie never shuffles. You are left out unless `listed`, so
/// Hide Me takes effect before the server answers.
pub fn ranked(people: &[Person], ranking: Ranking, listed: bool) -> Vec<&Person> {
    let mut rows: Vec<&Person> = people
        .iter()
        .filter(|person| listed || !person.you)
        .collect();
    rows.sort_by_key(|person| std::cmp::Reverse(person.standing.score(ranking)));
    rows
}

/// The rarest of these cans, and of those the newest.
pub fn best(cans: &[Can]) -> Option<&Can> {
    cans.iter().max_by_key(|can| can.rarity())
}

/// One beat of a card being opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Ack,
    /// The back glows this rarity's colour: the card has already beaten everything below it.
    Rung(Rarity),
    /// The light drops for a moment before the turn.
    Hush(Rarity),
    Turn,
    Hit,
    Settle,
}

impl Beat {
    pub fn face_up(self) -> bool {
        matches!(self, Self::Turn | Self::Hit | Self::Settle)
    }
}

/// A rarity's reveal, as beats and how long each lasts in milliseconds. Every card starts the
/// same, and each further beat is a real promotion: a card never shows a colour above its rarity.
pub fn ladder(rarity: Rarity) -> &'static [(Beat, u32)] {
    use Beat::{Ack, Hit, Hush, Rung, Settle, Turn};
    use Rarity::{Common, Legendary, Mythic, Rare, Uncommon};
    match rarity {
        Common => &[(Ack, 160), (Turn, 240), (Settle, 300)],
        Uncommon => &[
            (Ack, 160),
            (Rung(Uncommon), 360),
            (Turn, 240),
            (Settle, 440),
        ],
        Rare => &[
            (Ack, 160),
            (Rung(Uncommon), 360),
            (Rung(Rare), 580),
            (Turn, 240),
            (Hit, 360),
            (Settle, 600),
        ],
        Mythic => &[
            (Ack, 160),
            (Rung(Uncommon), 360),
            (Rung(Rare), 580),
            (Rung(Mythic), 700),
            (Hush(Mythic), 150),
            (Turn, 240),
            (Hit, 460),
            (Settle, 750),
        ],
        Legendary => &[
            (Ack, 160),
            (Rung(Uncommon), 360),
            (Rung(Rare), 580),
            (Rung(Mythic), 700),
            (Rung(Legendary), 900),
            (Hush(Legendary), 250),
            (Turn, 300),
            (Hit, 650),
            (Settle, 1300),
        ],
    }
}

/// A card being opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reveal {
    pub id: u64,
    pub identity: String,
    pub rarity: Rarity,
    pub step: usize,
}

impl Reveal {
    pub fn beat(&self) -> Beat {
        ladder(self.rarity)[self.step].0
    }

    /// How long the current beat lasts.
    pub fn delay(&self) -> StdDuration {
        StdDuration::from_millis(u64::from(ladder(self.rarity)[self.step].1))
    }
}

/// What the last try at adding or removing a can came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Added(Rarity),
    Removed,
    Failed(String),
}

/// A step of signing in to the collection server, shown in the page's place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Email,
    /// The code sent to `email`. `wrong` after one that didn't match, with how many more tries
    /// the server allows it; `resent` once a new code was asked for.
    Code {
        email: String,
        wrong: bool,
        tries_left: Option<u32>,
        resent: bool,
    },
    /// The passkey page for `ticket`. `create` when the passkey is being made, after the code;
    /// otherwise it signs in with one made before. `waiting` while the page is open in the
    /// browser and the app asks the server whether it's done.
    Passkey {
        ticket: String,
        create: bool,
        waiting: bool,
    },
    /// The TeamSpeak identities found on this computer, offered for linking.
    Identity(Vec<FoundIdentity>),
}

/// A TeamSpeak identity found on this computer: Flexspeak's own, or one an installed TeamSpeak
/// client keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundIdentity {
    /// The app it was found in.
    pub source: &'static str,
    /// The nickname it was last used with, if known.
    pub name: String,
    pub uid: String,
    pub linked: bool,
}

impl Gate {
    /// Its place among the four steps.
    pub fn step(&self) -> usize {
        match self {
            Self::Email => 0,
            Self::Code { .. } => 1,
            Self::Passkey { .. } => 2,
            Self::Identity(_) => 3,
        }
    }
}

/// The page's state. Empty until the page first opens.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The collection server's address, from Settings; `None` keeps the collection in
    /// `monster.sqlite` on this computer, with no account and no leaderboard.
    pub server: Option<String>,
    /// The session while signed in to the server.
    pub session: Option<String>,
    /// Whether the session kept from the last launch was read yet.
    pub session_read: bool,
    /// The account signed in, as the server last described it.
    pub me: Option<api::Me>,
    /// The sign-in step in the page's place; `None` once signed in, and without a server.
    pub gate: Option<Gate>,
    /// A sign-in request is out, so its buttons wait for the answer.
    pub busy: bool,
    /// Why the last sign-in request failed, in the server's words.
    pub gate_problem: Option<String>,
    /// Which passkey poll is the current one; Cancel moves it on, so a late answer is dropped.
    poll: u64,
    /// Flexspeak's own TeamSpeak identity, offered at the last sign-in step.
    pub own_identity: Option<FoundIdentity>,
    /// The cans `monster.sqlite` holds, offered to the server once after signing in.
    pub local: Vec<Can>,
    local_read: bool,
    /// The collection is on its way from the server.
    pub loading: bool,
    /// A new can is on its way to the server; taps wait for it.
    pub adding: bool,
    /// Everyone on the leaderboard, the person signed in among them.
    pub people: Vec<Person>,
    /// The leaderboard is on its way.
    pub people_loading: bool,
    /// Why the leaderboard, or hiding from it, failed.
    pub people_problem: Option<String>,
    /// The board of someone opened from the leaderboard.
    pub visit: Option<Visit>,
    pub ranking: Ranking,
    /// Whether the person signed in shows on the leaderboard, and can be opened by others.
    pub hidden: bool,
    /// Oldest first.
    pub cans: Vec<Can>,
    /// What the flavour search holds.
    pub query: String,
    /// The card being opened, while one is.
    pub reveal: Option<Reveal>,
    reveals: u64,
    pub notice: Option<Notice>,
    /// The card shown large; the newest if none was picked.
    pub selected: Option<String>,
    /// How many of the newest cards a grid shows.
    pub shown: usize,
    pub section: Section,
    /// Why the file couldn't be read. Saving is refused, so the file isn't overwritten.
    pub load_error: Option<String>,
    /// `None` until the page opens, and if the database couldn't be read.
    db: Option<Connection>,
    /// The roll key; made in memory when there's no database, as in previews.
    key: Option<[u8; 32]>,
    /// What the page asked of the server, for tests, which run nothing.
    #[cfg(test)]
    pub sent: Vec<Effect>,
}

impl Tracker {
    /// Reads the collection the first time the page opens. With a server, only reads the cans
    /// kept on this computer, to offer them to it once; the file isn't created or written.
    pub fn open(&mut self, settings: Option<&Path>) {
        self.shown = self.shown.max(PAGE);
        self.section = Section::Overview;
        if self.server.is_some() {
            if !self.local_read
                && let Some(settings) = settings
            {
                self.local_read = true;
                self.local = read_local(&path(settings)).unwrap_or_else(|error| {
                    tracing::warn!("The cans on this computer can't be offered: {error:#}");
                    Vec::new()
                });
            }
            return;
        }
        if self.db.is_some() {
            return;
        }
        let Some(settings) = settings else {
            return;
        };
        let opened = open_database(&path(settings)).and_then(|connection| {
            let key = roll_key(&connection)?;
            let cans = read_collection(&connection)?;
            Ok((connection, key, cans))
        });
        match opened {
            Ok((connection, key, cans)) => {
                self.cans = cans;
                self.key = Some(key);
                self.db = Some(connection);
                self.load_error = None;
            }
            Err(error) => self.load_error = Some(format!("{error:#}")),
        }
    }

    /// Records a can of this flavour, drunk at `at` (Unix seconds), and rolls its card.
    pub fn add(&mut self, flavour: usize, at: i64) {
        if self.load_error.is_some() {
            return;
        }
        let Some(flavour) = FLAVOURS.get(flavour) else {
            return;
        };
        let key = match self.key {
            Some(key) => key,
            None => match new_key() {
                Ok(key) => *self.key.insert(key),
                Err(error) => {
                    self.notice = Some(Notice::Failed(format!("{error:#}")));
                    return;
                }
            },
        };
        let number = self.cans.last().map_or(1, |can| can.id + 1);
        let (rarity, seed) = draw(&key, flavour.id, at, number);
        let mut can = Can {
            id: number,
            flavour: flavour.id.to_owned(),
            at,
            rarity,
            seed,
        };
        match self.record(&can) {
            Ok(Some(id)) => can.id = id,
            Ok(None) => {}
            Err(error) => {
                self.notice = Some(Notice::Failed(format!("{error:#}")));
                return;
            }
        }
        self.selected = Some(can.identity());
        self.cans.push(can);
        self.notice = Some(Notice::Added(rarity));
    }

    /// Saves a can; returns the number the database gave it.
    fn record(&self, can: &Can) -> Result<Option<i64>> {
        let Some(db) = &self.db else {
            return Ok(None);
        };
        db.execute(
            "INSERT INTO drinks (flavour, at, rarity, seed) VALUES (?1, ?2, ?3, ?4)",
            params![can.flavour, can.at, can.rarity.index() as i64, can.seed],
        )
        .context("Can't save monster.sqlite")?;
        Ok(Some(db.last_insert_rowid()))
    }

    /// Starts opening a saved card.
    pub fn start_reveal(&mut self, identity: String) -> Option<Reveal> {
        let rarity = self
            .cans
            .iter()
            .find(|can| can.identity() == identity)?
            .rarity();
        self.reveals += 1;
        self.selected = Some(identity.clone());
        let reveal = Reveal {
            id: self.reveals,
            identity,
            rarity,
            step: 0,
        };
        self.reveal = Some(reveal.clone());
        Some(reveal)
    }

    /// Moves to the next beat if that timer is still the current one; returns its length.
    pub fn advance_reveal(&mut self, id: u64) -> Option<StdDuration> {
        let reveal = self.reveal.as_mut().filter(|reveal| reveal.id == id)?;
        if reveal.step + 1 < ladder(reveal.rarity).len() {
            reveal.step += 1;
            Some(reveal.delay())
        } else {
            self.reveal = None;
            None
        }
    }

    /// The card still face down, whose rarity nothing on the page may give away yet.
    pub fn hidden_can(&self) -> Option<&str> {
        self.reveal
            .as_ref()
            .filter(|reveal| !reveal.beat().face_up())
            .map(|reveal| reveal.identity.as_str())
    }

    /// Takes a can out, for one added by mistake.
    pub fn remove(&mut self, identity: &str) {
        let Some(index) = self.cans.iter().position(|can| can.identity() == identity) else {
            return;
        };
        if let Some(db) = &self.db
            && let Err(error) = db
                .execute(
                    "DELETE FROM drinks WHERE id = ?1",
                    params![self.cans[index].id],
                )
                .context("Can't save monster.sqlite")
        {
            self.notice = Some(Notice::Failed(format!("{error:#}")));
            return;
        }
        self.cans.remove(index);
        self.selected = None;
        self.notice = Some(Notice::Removed);
    }

    /// One flavour's cans, oldest first.
    pub fn cans_of(&self, flavour: usize) -> Vec<Can> {
        self.cans
            .iter()
            .filter(|can| can.flavour_index() == flavour)
            .cloned()
            .collect()
    }

    /// The card shown large.
    pub fn featured(&self) -> Option<&Can> {
        self.selected
            .as_deref()
            .and_then(|identity| self.cans.iter().find(|can| can.identity() == identity))
            .or_else(|| self.cans.last())
    }
}

/// Everything the profile board shows, worked out from the cans.
#[derive(Debug, PartialEq)]
pub struct Board {
    pub total: u32,
    pub flavours: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub best_day: u32,
    /// Cans in the 365 days up to today.
    pub past_year: u32,
    pub litres: f32,
    pub caffeine_g: f32,
    pub days: BTreeMap<NaiveDate, u32>,
    /// Most-drunk first; ties by name.
    pub by_flavour: Vec<(&'static Flavour, u32)>,
    /// Indexed by `Rarity::index`.
    pub by_rarity: [u32; 5],
}

impl Board {
    pub fn new(cans: &[Can], today: NaiveDate, date_of: impl Fn(i64) -> NaiveDate) -> Self {
        let mut days = BTreeMap::new();
        let mut by_rarity = [0; 5];
        let mut flavours: BTreeMap<&str, u32> = BTreeMap::new();
        let mut seen = HashSet::new();
        for can in cans {
            *days.entry(date_of(can.at)).or_insert(0) += 1;
            by_rarity[can.rarity().index()] += 1;
            *flavours.entry(can.flavour().id).or_insert(0) += 1;
            seen.insert(can.flavour().id);
        }
        let mut by_flavour: Vec<(&'static Flavour, u32)> = flavours
            .into_iter()
            .map(|(id, count)| (flavour_or_other(id), count))
            .collect();
        by_flavour.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(b.0.name)));
        let (current_streak, longest_streak) = streaks(&days, today);
        let year_start = today - Duration::days(364);
        let total = cans.len() as u32;
        Self {
            total,
            flavours: seen.len() as u32,
            current_streak,
            longest_streak,
            best_day: days.values().copied().max().unwrap_or(0),
            past_year: days.range(year_start..=today).map(|(_, n)| n).sum(),
            litres: total as f32 * LITRES_PER_CAN,
            caffeine_g: (total * CAFFEINE_MG_PER_CAN) as f32 / 1000.0,
            days,
            by_flavour,
            by_rarity,
        }
    }
}

/// The run still alive and the longest ever, in days. As on a GitHub profile, a run that
/// reached yesterday is still alive: today isn't over.
fn streaks(days: &BTreeMap<NaiveDate, u32>, today: NaiveDate) -> (u32, u32) {
    let mut longest = 0;
    let mut run = 0;
    let mut last: Option<NaiveDate> = None;
    for &day in days.keys() {
        run = if last.is_some_and(|before| day - before == Duration::days(1)) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        last = Some(day);
    }
    let alive = last.is_some_and(|day| day == today || day == today - Duration::days(1));
    (if alive { run } else { 0 }, longest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Cans dated by day number from 2026-01-01, so `at` is the day.
    fn on_days(days: &[i64]) -> Vec<Can> {
        days.iter()
            .enumerate()
            .map(|(n, day)| Can {
                id: n as i64 + 1,
                flavour: "original".into(),
                at: *day,
                rarity: Rarity::Common,
                seed: 0,
            })
            .collect()
    }

    fn board(cans: &[Can], today: NaiveDate) -> Board {
        Board::new(cans, today, |at| date(2026, 1, 1) + Duration::days(at))
    }

    fn tracker(dir: &Path) -> Tracker {
        let mut tracker = Tracker::default();
        tracker.open(Some(&dir.join("settings.json")));
        tracker
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flexspeak-monster-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_odds_add_up_to_a_thousand() {
        let total: u32 = Rarity::ALL.iter().map(|r| r.permille()).sum();
        assert_eq!(total, 1000);
        assert_eq!(Rarity::from_roll(0), Rarity::Common);
        assert_eq!(Rarity::from_roll(599), Rarity::Common);
        assert_eq!(Rarity::from_roll(600), Rarity::Uncommon);
        assert_eq!(Rarity::from_roll(989), Rarity::Mythic);
        assert_eq!(Rarity::from_roll(990), Rarity::Legendary);
        assert_eq!(Rarity::from_roll(999), Rarity::Legendary);
    }

    #[test]
    fn a_large_batch_of_cans_lands_near_the_odds() {
        let key = [7; 32];
        let mut counts = [0u32; 5];
        for n in 0..50_000 {
            counts[draw(&key, "original", 1_700_000_000, n).0.index()] += 1;
        }
        for rarity in Rarity::ALL {
            let expected = rarity.permille() as f32 * 50.0;
            let got = counts[rarity.index()] as f32;
            assert!(
                (got - expected).abs() < expected * 0.15,
                "{rarity:?}: {got} against {expected}"
            );
        }
    }

    #[test]
    fn each_rarity_takes_the_researched_time_to_settle() {
        let settled = |rarity| ladder(rarity).iter().map(|(_, ms)| ms).sum::<u32>();
        assert_eq!(settled(Rarity::Common), 700);
        assert_eq!(settled(Rarity::Uncommon), 1200);
        assert_eq!(settled(Rarity::Rare), 2300);
        assert_eq!(settled(Rarity::Mythic), 3400);
        assert_eq!(settled(Rarity::Legendary), 5200);
    }

    #[test]
    fn a_card_never_shows_a_colour_above_its_rarity() {
        for rarity in Rarity::ALL {
            for (beat, _) in ladder(rarity) {
                if let Beat::Rung(shown) | Beat::Hush(shown) = beat {
                    assert!(*shown <= rarity, "{rarity:?} showed {shown:?}");
                }
            }
        }
    }

    #[test]
    fn every_reveal_starts_the_same_and_ends_face_up() {
        for rarity in Rarity::ALL {
            let beats = ladder(rarity);
            assert_eq!(beats[0], (Beat::Ack, 160));
            assert_eq!(beats[beats.len() - 1].0, Beat::Settle);
        }
    }

    #[test]
    fn a_reveal_steps_through_its_beats_and_ignores_old_timers() {
        let mut t = Tracker::default();
        t.add(0, 1);
        let identity = t.cans[0].identity();
        let reveal = t.start_reveal(identity.clone()).unwrap();
        assert_eq!(t.hidden_can(), Some(identity.as_str()));

        assert_eq!(t.advance_reveal(reveal.id + 1), None, "a stale timer");
        assert_eq!(t.reveal.as_ref().unwrap().step, 0);

        while t.advance_reveal(reveal.id).is_some() {
            if t.reveal.as_ref().unwrap().beat().face_up() {
                assert_eq!(t.hidden_can(), None);
            }
        }
        assert!(t.reveal.is_none(), "it finishes on its own");
    }

    #[test]
    fn only_a_saved_card_can_be_opened() {
        assert!(Tracker::default().start_reveal("nothing".into()).is_none());
    }

    #[test]
    fn a_can_is_saved_with_its_roll_and_comes_back_the_same() {
        let dir = temp_dir("saved");
        let mut t = tracker(&dir);
        t.add(2, 100);
        t.add(2, 100);
        assert_eq!(t.cans.len(), 2);
        assert_eq!(t.cans[0].flavour, FLAVOURS[2].id);
        assert_ne!(
            t.cans[0].id, t.cans[1].id,
            "two cans in one second are two cans"
        );
        assert_eq!(t.notice, Some(Notice::Added(t.cans[1].rarity)));
        assert_eq!(t.featured(), t.cans.last());
        assert_eq!(tracker(&dir).cans, t.cans);
    }

    #[test]
    fn the_roll_key_is_made_once_and_kept() {
        let dir = temp_dir("key");
        let first = tracker(&dir).key;
        assert!(first.is_some());
        assert_eq!(tracker(&dir).key, first);
        assert_ne!(
            tracker(&temp_dir("key-other")).key,
            first,
            "each collection has its own"
        );
    }

    #[test]
    fn the_roll_depends_on_the_key_and_the_can() {
        let roll = |key: u8, flavour, at, n| draw(&[key; 32], flavour, at, n);
        let base = roll(1, "original", 100, 1);
        assert_eq!(
            roll(1, "original", 100, 1),
            base,
            "the same inputs, the same card"
        );
        // Of 64 different cans, some must differ from the first; all equal would be no roll.
        assert!((2..66).any(|n| roll(1, "original", 100, n) != base));
        assert!((2..66).any(|key| roll(key, "original", 100, 1) != base));
    }

    #[test]
    fn a_scanned_collection_moves_across_with_its_cards() {
        let dir = temp_dir("migrate");
        let file = path(&dir.join("settings.json"));
        let old = Connection::open(&file).unwrap();
        old.execute_batch(
            "CREATE TABLE cans (id INTEGER PRIMARY KEY, barcode TEXT NOT NULL, code TEXT NOT NULL,
                 copy INTEGER NOT NULL DEFAULT 0, flavour TEXT NOT NULL, at INTEGER NOT NULL,
                 UNIQUE (barcode, code, copy));
             CREATE TABLE barcodes (barcode TEXT PRIMARY KEY, flavour TEXT NOT NULL);
             INSERT INTO cans (barcode, code, copy, flavour, at)
                 VALUES ('5901234123457', 'L24123A', 0, 'mango-loco', 10),
                        ('5901234123457', 'L24123A', 1, 'mango-loco', 20),
                        ('', 'OLDCODE1', 0, 'original', 5);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(old);
        let t = tracker(&dir);
        assert_eq!(t.load_error, None);
        assert_eq!(t.cans.len(), 3);
        assert_eq!(t.cans[1].at, 20);
        assert_eq!(
            (t.cans[0].rarity, t.cans[0].seed),
            legacy_roll("5901234123457", "L24123A", 0),
            "the card it was opened with"
        );
        assert_eq!(
            (t.cans[2].rarity, t.cans[2].seed),
            legacy_roll("", "OLDCODE1", 0)
        );
        // Opening it again moves nothing twice; the old tables stay as a backup.
        assert_eq!(tracker(&dir).cans.len(), 3);
        let db = Connection::open(&file).unwrap();
        let kept: i64 = db
            .query_row("SELECT COUNT(*) FROM scanned_cans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 3);
    }

    #[test]
    fn a_damaged_file_is_reported_and_never_overwritten() {
        let dir = temp_dir("damaged");
        let file = path(&dir.join("settings.json"));
        fs::write(&file, b"this is not a database at all").unwrap();
        let mut t = tracker(&dir);
        assert!(t.load_error.is_some());
        t.add(0, 100);
        assert!(t.cans.is_empty());
        assert_eq!(fs::read(&file).unwrap(), b"this is not a database at all");
    }

    #[test]
    fn a_can_that_cant_be_saved_isnt_counted() {
        let dir = temp_dir("unsaved");
        let mut t = tracker(&dir);
        // A database without its tables, so every write fails.
        t.db = Some(Connection::open_in_memory().unwrap());
        t.add(0, 100);
        assert!(t.cans.is_empty());
        assert!(matches!(t.notice, Some(Notice::Failed(_))));
    }

    #[test]
    fn a_removed_can_is_gone_for_good_and_its_number_isnt_reused() {
        let dir = temp_dir("remove");
        let mut t = tracker(&dir);
        t.add(0, 100);
        t.add(0, 200);
        let first = t.cans[0].identity();
        t.remove(&first);
        assert_eq!(t.notice, Some(Notice::Removed));
        assert_eq!(tracker(&dir).cans.len(), 1);
        t.add(0, 300);
        assert_ne!(t.cans.last().unwrap().identity(), first);
    }

    #[test]
    fn favourites_come_first_then_the_latest_then_the_usual_order() {
        let mut cans = on_days(&[0, 1, 2, 3]);
        cans[0].flavour = "mango-loco".into();
        cans[1].flavour = "mango-loco".into();
        cans[2].flavour = "ultra-white".into();
        cans[3].flavour = "pacific-punch".into();
        let order: Vec<&str> = favourites(&cans, "")
            .iter()
            .map(|(index, _)| FLAVOURS[*index].id)
            .collect();
        assert_eq!(&order[..3], ["mango-loco", "pacific-punch", "ultra-white"]);
        assert_eq!(order[3], FLAVOURS[0].id, "never drunk: as listed");
        assert_eq!(order.len(), FLAVOURS.len());
        assert_eq!(favourites(&cans, "")[0].1, 2);
    }

    #[test]
    fn a_search_keeps_flavours_whose_name_holds_every_word() {
        let names = |query| -> Vec<&str> {
            favourites(&[], query)
                .iter()
                .map(|(index, _)| FLAVOURS[*index].name)
                .collect()
        };
        assert_eq!(names("MANGO"), ["Ultra Fiesta Mango", "Mango Loco"]);
        assert_eq!(names("ultra mango"), ["Ultra Fiesta Mango"]);
        assert!(names("no such drink").is_empty());
        assert_eq!(names("  ").len(), FLAVOURS.len());
    }

    #[test]
    fn a_saved_can_of_a_dropped_flavour_still_shows() {
        let can = Can {
            id: 1,
            flavour: "discontinued".into(),
            at: 0,
            rarity: Rarity::Common,
            seed: 0,
        };
        assert_eq!(can.flavour().id, "other");
        assert_eq!(can.flavour_index(), FLAVOURS.len() - 1);
    }

    #[test]
    fn every_flavour_has_a_unique_id() {
        let ids: HashSet<_> = FLAVOURS.iter().map(|f| f.id).collect();
        assert_eq!(ids.len(), FLAVOURS.len());
    }

    #[test]
    fn a_streak_stays_alive_until_a_whole_day_is_missed() {
        let cans = on_days(&[0, 1, 2, 5, 6]);
        let last = date(2026, 1, 7);
        assert_eq!(board(&cans, last).current_streak, 2, "drank today");
        assert_eq!(
            board(&cans, last + Duration::days(1)).current_streak,
            2,
            "today isn't over"
        );
        assert_eq!(
            board(&cans, last + Duration::days(2)).current_streak,
            0,
            "yesterday was missed"
        );
        assert_eq!(board(&cans, last).longest_streak, 3);
    }

    #[test]
    fn the_board_totals_cans_days_and_flavours() {
        let mut cans = on_days(&[0, 0, 0, 1]);
        cans[1].flavour = "ultra-white".into();
        let b = board(&cans, date(2026, 1, 2));
        assert_eq!(b.total, 4);
        assert_eq!(b.flavours, 2);
        assert_eq!(b.best_day, 3);
        assert_eq!(b.past_year, 4);
        assert_eq!(b.litres, 2.0);
        assert_eq!(b.caffeine_g, 0.64);
        assert_eq!(b.by_flavour[0].0.id, "original");
        assert_eq!(b.by_flavour[0].1, 3);
        assert_eq!(b.by_rarity.iter().sum::<u32>(), 4);
    }

    #[test]
    fn the_past_year_stops_at_365_days() {
        let cans = on_days(&[0, 365]);
        let today = date(2026, 1, 1) + Duration::days(365);
        assert_eq!(board(&cans, today).past_year, 1);
        assert_eq!(board(&cans, today - Duration::days(1)).past_year, 1);
    }

    #[test]
    fn an_empty_collection_has_an_empty_board() {
        let b = board(&[], date(2026, 1, 1));
        assert_eq!((b.total, b.current_streak, b.longest_streak), (0, 0, 0));
    }

    #[test]
    fn a_flavour_keeps_only_its_own_cans_in_order() {
        let mut t = Tracker {
            cans: on_days(&[0, 1, 2]),
            ..Default::default()
        };
        t.cans[1].flavour = "ultra-white".into();
        let (white, _) = flavour_by_id("ultra-white").unwrap();
        let (original, _) = flavour_by_id("original").unwrap();
        assert_eq!(t.cans_of(white).len(), 1);
        let cans = t.cans_of(original);
        assert_eq!(cans[0].id, t.cans[0].id);
        assert_eq!(cans[1].id, t.cans[2].id);
    }

    fn rolling(rarity: Rarity, n: usize, at: i64) -> Can {
        Can {
            id: n as i64 + 10 * rarity.index() as i64,
            flavour: "original".into(),
            at,
            rarity,
            seed: 0,
        }
    }

    #[test]
    fn the_best_card_is_the_rarest_and_then_the_newest() {
        assert!(best(&[]).is_none());
        let rare = rolling(Rarity::Rare, 0, 1);
        let newer_rare = rolling(Rarity::Rare, 1, 2);
        let cans = [rolling(Rarity::Common, 0, 0), rare, newer_rare.clone()];
        assert_eq!(best(&cans), Some(&newer_rare));
        let mythic = rolling(Rarity::Mythic, 0, 0);
        let cans = [mythic.clone(), newer_rare];
        assert_eq!(best(&cans), Some(&mythic), "rarity beats age");
    }

    #[test]
    fn back_climbs_from_a_flavour_to_the_list_to_the_overview() {
        assert_eq!(Section::Flavour(3).parent(), Some(Section::Flavours));
        assert_eq!(Section::Flavours.parent(), Some(Section::Overview));
        assert_eq!(Section::Overview.parent(), None);
    }
}
