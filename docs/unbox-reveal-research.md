# Unbox reveal research

Date: **2026-11-09**. Research only; no source code changed. The durations, synthesis numbers and effect choices in section 5 are my design proposal. No study or shipped game specifies them, and none has been built or play-tested.

Evidence labels used below:

- **Replicated**: found independently by several research groups or in a large replication or meta-analysis.
- **Documented**: stated by the source that owns the fact (a paper's own results, a regulator, a developer's own post, the repo's code).
- **Mixed**: credible studies disagree.
- **Inferred**: my reasoning from documented facts. No source states it.
- **Unverified**: rests only on fan wikis, blogs or press summaries.
- **Not established**: I looked and could not find it.
- **(excerpt)**: the page refused automated reading, so the text comes from a search-index excerpt of that page.

## Short answer

**Make it delightful, and don't build for compulsion.** The app already has the strongest hook there is: an unpredictable reward on every can, which the hash supplies. What's left to add is craft. Each reveal should climb a ladder of rungs in which every extra beat is a real promotion, and Commons should be over almost at once.

The word "addictive" needs one honest correction. In most games the repeated action is a click. Here it is drinking 160 mg of caffeine, and the only way to get another roll is another can. So the techniques that raise repeat behaviour most (streaks, progress toward the next rare, pity timers, daily rewards) are the ones to leave out or remove. The app's existing **Current Streak** counter is the first thing I would change.

| Conclusion | Confidence |
| --- | --- |
| Unpredictable reward already exists through the hash. It needs no strengthening. | High |
| Anticipation before a reveal raises arousal, and rarer outcomes raise arousal and urge. | Medium-high: one direct loot-box lab paper, plus consistent adjacent work |
| Reward reactions move to the earliest reliable cue, so tells should reveal rarity in steps. | High for the neuroscience in animals; medium as a design inference |
| Common outcomes must be short, and repeat viewers need a skip. | High as shipped-game practice; no controlled study found |
| Near misses and fake-outs: leave out. | High as an ethics call. The evidence that they cause persistence is mixed. |
| Win sounds raise arousal, make people overestimate wins, and can increase risky choice. | High for arousal and overestimation; medium for risky choice |
| Paid loot-box findings and gambling law mostly do not apply to a free, private tracker. | High |
| Daily streaks for a caffeinated drink are the real risk here. | High that logged streaks increase repeat behaviour; **not established** for energy drinks specifically |
| The per-rarity timings in section 5. | Low-medium: reasoned from the above, untested |

## Scope and how the app works today

These facts come from the `feature/monster-tracker` branch.

- Rarity comes only from SHA-256 of the can's identity, so retyping can't reroll it. The odds per 1000 are 600, 250, 100, 40 and 10 (`roll`, `Rarity::from_roll` in `src/monster.rs`). The app already shows these odds.
- `Tracker::add` saves the can before setting `Notice::Added(rarity)`. A skipped animation therefore can't lose a card.
- The app counts a **Current Streak** and **Longest Streak** by calendar day (`streaks` in `src/monster.rs`, shown in `src/ui/monster.rs`), and a lifetime **Caffeine** total at a flat 160 mg per can (`CAFFEINE_MG_PER_CAN`).
- Each flavour page shows a dot for each rarity pulled (`src/ui/monster.rs`), which is already a light completion mechanic.

*My arithmetic, from those odds and the 27 entries in `FLAVOURS`:* one can in 6.7 is Rare or better, one in 20 is Mythic or better, and one in 100 is Legendary. If flavours were equally likely, collecting all 27 Legendaries would take about 10,500 cans (2700 × H₂₇), or about 29 years at one can a day and 1.7 kg of caffeine. Completing the grid is not a reachable goal, and the app should never present it as one.

Rune and rune-fx facts checked at the pinned revision (`dece2af`, `crates/rune-fx/src`):

- One-shot effects exist: `SparkleParams.burst`, `PulseStyle::Ripple` with `t0` and `duration`, `DistortParams::Shockwave`, and `ElectricParams.zap` (0.5 s). `Heat` eases a light on and off, and `Phase` changes speed without a jump.
- `Fx::Distort` reads the backdrop, and the app's compositor currently passes `backdrop: None` (`src/ui/fx.rs`). **Shockwave needs `set_layer_backdrop` wiring first.**
- Card layers are drawn with `.at(time + seed % 97)`. A one-shot's `t0` must use the same offset, or it will fire at the wrong moment.

Two app constraints that shape the design, both found while reading the code:

- **`AudioHub::play` is silent unless a server has opened the audio device.** Its own comment says so (`src/hub.rs`). A reveal while disconnected would make no sound without a separate output path.
- **`AudioEngine::play` replaces the current sound** (`self.chime = samples.into()` in `src/audio.rs`). Each reveal must be one pre-mixed buffer, not several calls.

Access limits: EFSA's journal page and TouchArcade returned HTTP 403, the European Parliament's text returned an empty page, and Fandom and wiki.gg refused automated reads. I used EFSA's own topic page and brochure in place of the journal text. I could not watch video, so conference talks are cited only for what their listings or written coverage state.

## 1. Reward psychology

| Mechanism | What it is established to do | Strength | Use here |
| --- | --- | --- | --- |
| Reward prediction error | Dopamine neurons fire for better-than-expected rewards, not for fully predicted ones, and dip when an expected reward is withheld. The response **moves stepwise to the earliest reliable predictor**. [Schultz 1997][schultz97], [Schultz 2016][schultz16] | **Replicated** in animals. Human evidence is less direct. | Tells should release information in steps. |
| Uncertainty ramp | Fiorillo et al. saw a sustained dopamine rise until the reward time, largest at p = 0.5; 29% of 188 neurons showed it. [Fiorillo 2003][fiorillo-pdf] | **Mixed**. Niv et al. showed it could be an averaging artefact. [Niv 2005][niv] Hart et al. later measured a variance-linked signal in rats. [Hart 2015][hart] | Do not design around "50% is most exciting". That is a popular extrapolation. |
| "Dopamine is pleasure" | Reviews argue dopamine is neither necessary nor sufficient for "liking", and fits "wanting" better. [Berridge 2007][berridge] | Popular claim, contested | Wanting more is not the same as enjoying more. |
| Variable-ratio reinforcement | Intermittently rewarded behaviour persists longer without reward (the partial reinforcement extinction effect). One human study found it larger in frequent gamblers, n = 40. [Horsley 2012][horsley] Slot machines are strictly random-ratio, each spin independent. [Haw 2008][haw] **(excerpt)** | Classic and strong in animals. Human gambling evidence is thinner. | The hash roll is random-ratio per can. It is already present. |
| Anticipation | In a loot-box lab study, skin conductance rose before the reveal. [Larche 2021][larche] People may prefer later resolution of a real lottery because of hope and thrill. [Kocher 2014][kocher] Music caused dopamine release in anticipation and at the peak, in distinct regions. [Salimpoor 2011][salimpoor] Drug manipulation changed musical pleasure in a double-blind study, n = 27. [Ferreri 2019][ferreri] | Consistent, small samples | A short build is justified. |
| Rarity and urge | Rarer Overwatch boxes produced more arousal, more reward reactivity and more urge to open another; 47 valid participants in experiment 1, watching videos. [Larche 2021][larche] | Documented, one lab | Escalate by rarity. Expect it to raise urge too. |
| Near misses | Raised desire to play, recruited win circuitry, and felt less pleasant. [Clark 2009][clark] Stronger midbrain response with gambling severity. [Chase & Clark 2010][chase] One study found greater persistence. [Kassinove 2001][kassinove] Large arousal, but no reward-style pause. [Dixon 2013][dixon13] A review and experiments with pigeons and humans **failed to support** an effect on persistence. [Pisklak 2020][pisklak] | **Mixed**. Brain and self-report effects are solid; persistence is disputed. | Leave out. See section 4. |
| Goal gradient | Café customers bought faster as a reward neared, and an "illusory head start" also sped them up. Purchase rates reset after the reward. [Kivetz 2006][kivetz] Motivation can dip halfway. [Bonezzi 2011][bonezzi] | Documented field study. **I found no large preregistered replication.** The much-repeated 34% vs 19% car-wash figure I could only find on [popular pages][coglode]. **Unverified.** | Do not add a progress bar toward the next rare. It would accelerate cans. |
| Collection completion | People prefer sets that "fit", even choosing worse items. [Evers 2014][evers] Japan outlawed paid "complete gacha". [law-firm summary][monolith] | **I found no controlled study showing that card-set completion drives repeat buying.** "Completionism" is a popular explanation. | Show the collection. Attach no reward to finishing it. |
| Streaks | Across seven studies, intact logged streaks raised later engagement, **regardless of actual past behaviour**, because the log itself becomes a goal. Repairing a break weakened the effect. [Silverman & Barasch 2023][silverman] | Documented, journal paper. Effect sizes not checked beyond the abstract. | Applies directly. See section 4. |
| Loss aversion | Prospect theory patterns replicated for 94% of items across 19 countries. [Ruggeri 2020][ruggeri] Duolingo says it deliberately "taps into" this for long streaks. [Duolingo][duolingo] | **Replicated** for risky money choices. The extension to streaks is plausible and first-party only. | Never frame a streak as something to lose. |

Two further results matter for this app:

- **Variable prizes change real behaviour.** Prize draws for drug-free tests raised abstinence (d = 0.46 across 19 trials), but the effect had vanished at 6 months. [Benishek 2014][benishek] *Inferred:* the same lever pointed at consumption could push the other way.
- **Tracking is not neutral.** Self-monitoring explained the most variation in health-behaviour interventions. [Michie 2009][michie] Six experiments found that measuring an activity increased how much people did and reduced how much they enjoyed it. [Etkin 2016][etkin], [Duke listing][etkin-duke]

## 2. Reveal craft

**The main gap:** I found no GDC talk or developer post that gives a pack-reveal timing table, for any game. Blizzard's 2015 Hearthstone UI talk exists, but its [listing][gdc-hs] and [written coverage][gd-hs] say nothing about packs, and I could not watch it. Most of what follows is qualitative.

| Game | What is documented | Quality |
| --- | --- | --- |
| Overwatch | A Blizzard designer: "we want to build anticipation" through animation, camera, spinning plates, sound and glow from the box's cracks. The team **pulled back the original colourful light show because it killed anticipation**. [Game Developer on Kotaku][heiberg] The Belgian regulator describes discs showing rarity by colour before the items appear: "The tension is built up in multiple steps." [Belgian report][belgium] The videos in one study shake for about 2 s, release coins at about 2 s, and show items at 5 s. [Larche 2021][larche] | Developer quote; regulator; measured from study stimuli |
| Hearthstone | The art director: each set gets its own opening, and the team dropped ribbons and flaps: "let's just blow the thing up". [TouchArcade][toucharcade] **(excerpt)** Pack lifts, cracks, explodes; cards are face down and turned one by one, with rarity shown by coloured glow on hover. [Community wiki][hs-wiki] **(excerpt, unverified)** Blizzard added hold-Space opening: "Want it to be fast, but never accidentally hit." [Patch note quoted by Out of Games][outof] | Developer via press; community wiki; quoted patch note |
| Pokémon TCG Pocket | Lead developer: small touches like flipping a card and giving it thickness make it feel physical, "also how we think about the pack-opening experience". [Google Play interview][nagaya] | Developer interview. Skip behaviour **not established**. |
| Marvel Snap | Ben Brode on random card rewards: "We want it to be surprising and exciting, but also fair", with bad-luck protection. [Brode's post][brode] | Developer post, about rewards rather than animation |
| EA FC | EA **shortened** the reveal sequence for most high-rated items after beta feedback. [EA v1.1 notes][ea-v11] FIFA 21 Preview Packs could be watched or skipped. [EA][ea-preview] Press reports a 25-second FC 27 sequence revealing position, then nation, then club, and a backlash about repetition. [RetroGems][retrogems] **Unverified** | First-party on shortening and skip; press for the rest |
| Genshin Impact | A coloured star as the tell. [Blog][hostedgg] **Unverified.** Skip behaviour and official colour meanings **not established**. FTC action concerned odds and costs, not the animation. [FTC][ftc-gi] | Blog only |
| Counter-Strike | New York alleges an unlocking animation, then a wheel that "spins rapidly at first and then gradually slows" and may stop next to a rare item, a "near miss", while the result is set by a server-side random number. [NY AG complaint][nyag] Duration **not established**. Valve has published nothing I could find. | Legal allegation, not a finding |

What the sources support for this app:

1. **Reveal rarity in steps.** Position → nation → club, and discs before items, are both staged disclosure. This matches the stepwise transfer in the dopamine work (*inferred link*).
2. **Too much light too early defeats the build.** Overwatch's designers said so directly.
3. **Length is the most-complained-about property.** EA cut its reveal after feedback, and Blizzard added fast opening.
4. **Skips belong in the design from the start.** Players ration the full experience for pulls that matter. [Blog][hostedgg] **Unverified**, but it matches the shipped skip options above.
5. **A physical feel matters more than effects count**, per Pocket's developers and the Overwatch cutback.

Game feel:

- Swink defines polish as "the interactive impression of physicality created by the harmony of animation, sounds, and effects with input-driven motion". [Swink][swink]
- *Juice It or Lose It* is listed at GDC Europe 2012, and shows tricks applied live to a plain game. [GDC Vault][juice] Nijman's [*The Art of Screenshake*][screenshake] takes a dull shooter through 30 small tweaks. [Game Developer][vlambeer] I could not watch either talk, so I am not attributing specific techniques to them.
- More is not better. A survey of over 200 game-feel sources reports that medium and high juice beat both none and extreme. [Pichlmair & Johansen][gamefeel-survey] I read this through the survey, not Kao's original study.
- Duolingo reports that a new animation after a streak raised 7-day retention of new learners by 1.7%. [Duolingo][duolingo] It is the only first-party measured effect of a celebration animation I found, and it is a retention number, not a measure of delight.

Timing limits: 0.1 s feels instant, 1 s keeps flow, and 10 s is the limit of attention. [Nielsen][nng]

## 3. Sound

**What reward sounds do:**

- With win sounds, 96 gamblers showed higher arousal, mostly preferred the sound version, and overestimated how often they had won. [Dixon 2014][dixon14]
- Slot machines celebrate spins that actually lost money, and these "losses disguised as wins" were as arousing as real wins. [Dixon 2010][dixon10]
- Adding win-paired audiovisual cues made rats choose riskier options. [Barrus 2016][barrus] In 131 people, cues made lottery choices riskier and reduced attention to probabilities, though not on the Iowa task. [Cherkasova 2018][cherkasova]

*Inferred for this app:* every can yields a card, so the risk is celebrating everything equally. A Common should sound like a small, honest result. There is no money at stake to misjudge, so the risky-choice findings carry over only loosely.

**How to build the escalation.** Rising pitch for positive events is a strong convention, and the repo already follows it ("Rising means on and falling means off", `src/audio.rs`). **I found no study testing rising stingers for rewards**, so treat it as craft. Musical anticipation then release has the neuroscience above behind it.

**Practical synthesis, no licensed assets:**

- Reuse `soft_note` and `Voice` in `src/audio.rs`. They are 48 kHz additive voices with an exponential decay per partial.
- `Voice::Bell` already follows Risset's observation that higher partials decay faster: ratios 1, 2.76 and 5.4 with decays 5, 9 and 18. [CCRMA][ccrma]
- Only two things are missing: filtered noise for a riser and a low sine thump. Each is a few lines.
- Peak level should stay at or below 0.28, matching the app's loudest cue (`Cue::Poke`). The Soft pack sits at 0.07 to 0.16.
- All of this is generated, so it stays clear of the rule that **TeamSpeak's sounds must never be bundled**. Reveal sounds should not come from the local TeamSpeak bank either.
- WCAG's audio rule targets sound over 3 seconds that starts without user action. [WCAG 1.4.2][wcag-audio] A reveal is user-triggered, but it should still respect `sound_cues` and `volume_percent`.

## 4. Limits and harm

### What researchers find

- Loot-box spending correlates with problem gambling: η² = 0.054 in 7,422 gamers, [Zendle & Cairns 2018][zendle18]; a similar result in a replication, [2019][zendle19]; r = .27 across studies, [Spicer 2022][spicer]; r = .26, [Garea 2021][garea] **(excerpt)**. **Causation is not established**, as the authors say. Predatory monetization is a recognised concern. [King & Delfabbro 2018][king]
- **Physical card packs differ.** In 726 card-game players, spending on real booster packs had no significant link to problem gambling. [Zendle 2021][zendle21] Similar structure does not automatically mean similar harm.

### What regulators say

- **Belgium, 2018:** a game of chance needs a bet, a win or loss, and chance. Paid boxes in FIFA 18, Overwatch and CS:GO qualified. It noted that even free boxes confront players with "animations that are very similar to animations used in slot machine operation". [Belgian report][belgium]
- **UK, 2022:** "robust evidence of an association" but no established causation. Legislation was judged premature in favour of [industry-led protections][uk-principles]. [UK response][uk2022]
- **Netherlands, 2022:** the Council of State ruled FIFA boxes were not a game of chance. [Law-firm summary][clifford-nl]
- **EU:** Parliament adopted a consumer-protection resolution in January 2023. [Law-firm summary][covington] I could not read the [official text][ep2023]. Consumer authorities said in September 2026 that there is "broad consensus" that some designs harm consumers, and they want people not to be tricked into spending "more money or time" than they want. [CPC statement][cpc] A Digital Fairness Act proposal was reported as expected; I could not confirm publication by this date. [Lewis Silkin][lewis]
- **US:** the FTC's $20M Genshin settlement alleged misleading odds and costs. [FTC][ftc-gi] New York sued Valve in 2026, alleging illegal gambling, a slot-like wheel and near misses. These are allegations. [NY AG][nyag-pr]
- **Dark-pattern taxonomies:** Zagal et al. defined [dark game design patterns][zagal], including ["playing by appointment"][zagal-mock]. The FTC's four categories concern false beliefs, hidden information, unauthorized charges and subverted privacy. [FTC report][ftc-dp] Mathur et al. [catalogued shopping sites][mathur].

### Which techniques are exploitative, and whether that applies here

| Technique | In commercial settings | In this free, private, offline tracker |
| --- | --- | --- |
| Paid random boxes, confusing currencies | Exploitative; the regulators' main target | **Does not apply.** No payment to the app. |
| Hidden or misleading odds | Deceptive; the FTC's complaint | **Does not apply.** Odds are shown and the hash has no hidden adjustment. Keep it so. |
| Engineered near misses | Exploitative; alleged against Valve | Applies if built. **Don't build it.** |
| Celebrating losses as wins | Exploitative | Partly: over-celebrating Commons would be a mild version. |
| Artificial scarcity, countdowns | Named by the CPC | Does not apply unless added. **Don't add.** |
| Daily appointments, streaks | A named dark pattern | **Applies, because the action is drinking.** |
| Social pressure, leaderboards | Common | Does not apply. No other users. |
| Harm to children | The core regulatory concern | **Partly applies.** See below. |
| Gambling law | Depends on a stake | *Inferred:* no stake, since the drink is consumed either way. Not legal advice. |

### The caffeine-specific problem

This is what makes this app different from every game above: the reward depends on consuming a stimulant.

Guidance:

- **EFSA 2015:** up to 400 mg a day from all sources raises no safety concern for healthy adults, except pregnant women. Single doses up to 200 mg are also fine. 100 mg near bedtime may affect sleep. Pregnant or lactating women: up to 200 mg a day. Children and adolescents: 3 mg/kg a day. [EFSA topic page][efsa-topic], [EFSA brochure][efsa-brochure]. The journal record ([DOI][efsa-doi]) refused automated reading.
- **FDA:** 400 mg a day is "not generally associated with negative effects" for most adults, with wide individual variation. Energy drinks range from 54 to 328 mg per 16 fl oz. Experts advise against them for children and teens. [FDA][fda]
- **Sleep:** 400 mg six hours before bed cut total sleep significantly. [Drake 2013][drake]
- **Dependence:** withdrawal is well characterised, and avoiding it plays "a central role in habitual caffeine consumption". [Juliano & Griffiths 2004][juliano]

For this app (*my arithmetic*, at the app's assumed 160 mg per can):

- One can is under the 200 mg single-dose figure. Two together exceed it.
- Two and a half cans reach 400 mg. **The third can of a day crosses it.**
- One can reaches EFSA's 3 mg/kg child and adolescent level for anyone under about 53 kg.

Evidence limits. **No study I found measures whether collectibles or streaks change energy-drink intake.** The closest case is Untappd, a beer check-in app with badges and streaks. A published [ethics analysis][untappd] says some badges encourage high-frequency or high-quantity drinking, but it did not measure drinking. So the risk is *inferred* from three documented facts: logged streaks raise repeat behaviour, variable prizes change real behaviour, and a daily caffeine routine has a withdrawal incentive of its own.

**Recommendations, in order of importance:**

1. **Remove Current Streak and Longest Streak.** A daily streak here literally counts consecutive days of drinking caffeine. If a replacement is wanted, show "Rest days this month". Streak freezes (Duolingo's fix) would keep the mechanic alive, so they don't help.
2. **No reward for quantity.** Rarity odds, reveal size and sound must never depend on streak length, cans per day or cans in a row.
3. **The reveal gets quieter past the guideline.** From the can that takes today's total over 400 mg, normally the third: log it, give the card its true rarity, play the Quick reveal with no sound, and show one neutral line, such as "480 mg today. EFSA's adult guideline is 400 mg." No red, no scolding, because shame teaches people to stop logging.
4. **Let the fun be free.** Add a **Replay reveal** on every card. It gives the enjoyable part without needing another can.
5. **Turn the caffeine number into information.** Lifetime grams reads like a high score. Show today's estimated milligrams instead, and say that the per-can figure is an estimate.
6. **No notifications, daily rewards, timed events, pity timers or next-rare progress bars.**
7. **Don't add anti-cheat.** A typed code can already be invented. That decouples the reward from drinking, which is a feature here.
8. **Keep it hidden.** If this page ever becomes discoverable in a public build, add an adults-only note, since Flexspeak's audience may include teens (*inferred*).

## 5. Recommendation

### The design: a ladder

Rarity is known the moment the can is submitted, so the animation is theatre and should be honest theatre.

- **Every reveal starts identically** for 160 ms.
- **Each rung is a promotion.** If the card turns over, it stops. If it warms up instead, it has already beaten Common.
- **The colour climbs through the rarity colours**: `0x57d38c`, `0x4da3ff`, `0xc08bff`, `0xffb02e` (`Rarity::color`). **A card never shows a colour above its true rarity**, so there are no fake-outs.
- A Common is never presented as an almost-anything. Every result is a card.

| Rarity | Share | Face visible | Fully settled |
| --- | --- | --- | --- |
| Common | 60% | 280 ms | 700 ms |
| Uncommon | 25% | 640 ms | 1,200 ms |
| Rare | 10% | 1,220 ms | 2,300 ms |
| Mythic | 4% | 2,070 ms | 3,400 ms |
| Legendary | 1% | 3,100 ms | 5,200 ms |

*Weighted mean: 1.14 s to settle.* Faces of 85% of cards appear in under 650 ms.

Notation for the tables. Times are ms from submit. Sound is `Voice · MIDI notes · step ms · gain`, played by `soft_note`. `k` is card width ÷ 236. All sound is rendered into one buffer. "Noise" is white noise through a one-pole low-pass. "Turn" is a crossfade from card back to face with a scale punch; Rune's `.scale()` is uniform, so a true one-axis flip would need a width animation.

### Common (60%)

| Stage | ms | rune-fx | Sound | Foreshadowing |
| --- | --- | --- | --- | --- |
| Ack | 0–160 | Card back: Art + Grain, `.scale` 0.94 → 1 | `Wood · 72 · – · 0.12` | None: same for all rarities |
| Turn | 160–400 | Crossfade to face, scale 1.02 → 1. Face at 280 | at 280: `Felt · 72, 76 · 90 · 0.09` | The card turning right away *is* the answer |
| Settle | 400–700 | `card_layers(Common)`: Art, Grain | tail only | – |

**Skip:** none needed. Any input completes it at once.

### Uncommon (25%)

| Stage | ms | rune-fx | Sound | Foreshadowing |
| --- | --- | --- | --- | --- |
| Ack | 0–160 | As Common | As Common | None |
| Rung 1 | 160–520 | `Fx::glow` Soft, `0x57d38c`, radius `20k`, `heat` via `Heat(attack 0.25 s)`, intensity 0.5 | at 160: `Glass · 79 · – · 0.07` | **First tell**: not Common |
| Turn | 520–760 | Crossfade; face at 640 | at 640: `Felt · 72, 76, 79 · 80 · 0.10` | Stops at rung 1 |
| Settle | 760–1200 | Final layers; one `Shimmer` pass at intensity 0.16 | tail | – |

**Skip:** any click, Enter, Space or Esc jumps to the settled card.

### Rare (10%)

| Stage | ms | rune-fx | Sound | Foreshadowing |
| --- | --- | --- | --- | --- |
| Ack, Rung 1 | 0–520 | As Uncommon | As Uncommon | Looks like Uncommon |
| Rung 2 | 520–1100 | Glow eases to `0x4da3ff`, intensity 0.8; `Sparkles` continuous, count 6 | at 520: `Glass · 84 · – · 0.08`. Noise 400 → 3000 Hz, gain 0 → 0.03 | Colour promotes to blue |
| Turn | 1100–1340 | Crossfade; face at 1220; noise stops | at 1220: `Bell · 72, 79, 84 · 75 · 0.12`, then `Glass · 88` | Stops at rung 2 |
| Hit | 1340–1700 | `SparkleParams { count: 24, life: 1.0, burst: Some(t) }`; `PulseStyle::Ripple` from centre, 0.5 s | bell tail | – |
| Settle | 1700–2300 | `card_layers(Rare)`; one Shimmer pass | tail | – |

**Skip:** cuts to settled card; audio fades over 30 ms.

### Mythic (4%)

| Stage | ms | rune-fx | Sound | Foreshadowing |
| --- | --- | --- | --- | --- |
| Ack to Rung 2 | 0–1100 | As Rare, noise continues | As Rare, noise rising to 0.05 by 1800 | Looks like Rare |
| Rung 3 | 1100–1800 | Glow to `0xc08bff`, Aura style, intensity 1.3. `ElectricParams::charged()`, amplitude `0 → 1.6k` | at 1100: `Bell · 88 · – · 0.10`; `Hum · 48 · – · 0.10` | Colour and arcs |
| Hush | 1800–1950 | `heat` → 0.5, arcs held | Noise cut with 30 ms fade; hum continues | The pause says something is coming |
| Turn | 1950–2190 | Crossfade; face at 2070 | at 2070: `Bell · 72, 76, 79, 84, 88 · 70 · 0.13`; `Hum · 48 · – · 0.12` | Stops at rung 3 |
| Hit | 2190–2650 | Burst, count 32; Ripple; one `zap` (0.5 s) | bell tail | – |
| Settle | 2650–3400 | `card_layers(Mythic)`; Shimmer pass | tail | – |

**Skip:** as Rare. Show a small "Skip" hint from 1100 ms on.

### Legendary (1%)

| Stage | ms | rune-fx | Sound | Foreshadowing |
| --- | --- | --- | --- | --- |
| Ack to Rung 3 | 0–1800 | As Mythic | As Mythic, noise rising to 0.07 by 2700 | Looks like Mythic |
| Rung 4 | 1800–2700 | Glow to `0xffb02e`; `FireStyle::Border`, height `0 → 26k`; Electric amplitude `→ 4k`; Sparkles 16; speeds raised through `Phase::set_speed` to ×2 | at 1800: `Bell · 91 · – · 0.10`; `Hum · 36 · – · 0.12` with +12 semitone glide | Gold and fire |
| Hush | 2700–2950 | `heat` → 0.25, fire falls to 40% | **Silence**, 30 ms fade in | The largest pause |
| Turn | 2950–3250 | Crossfade; face at 3100 | at 3100: sine 55 → 40 Hz, 180 ms, gain 0.22; noise burst 120 ms, gain 0.08; `Bell · 72, 79, 84, 88, 91, 96 · 65 · 0.14` | Stops at the top |
| Hit | 3250–3900 | Burst, count 64; `Shockwave { reach: 240, amount: 9, duration: 0.9 }` if the backdrop is wired, otherwise Ripple; one `zap`; card-only shake ≤ 4 pt for 140 ms | bells ring on | – |
| Settle | 3900–5200 | `card_layers(Legendary)`; gold Shimmer pass; the name fades in last | `Hum · 60, 67 · 200 · 0.08`, tail ending by 5200, mix peak ≤ 0.28 | – |

**Skip:** as Rare, but only from 400 ms on, so the Submit press itself can't skip it.

### Across all rarities

**Skip, Quick and accessibility:**

- Any click on the card, Enter, Space or Esc skips. The can was saved before the animation started.
- Setting **Reveal: Full / Quick / Off**. Quick is 240 ms: Ack, crossfade, and the final chord only. Off is today's notice.
- **Repeat logging:** after one reveal, further cans within 10 minutes use Quick for Common and Uncommon. Rare and above keep their ladder.
- **Reduced motion** (`cx.is_reduced_motion()`): no scaling, shake or Shockwave. `.reduced(true)` stills the layers. Crossfade over 200 ms. Colour steps may stay, at 300 ms or longer each. [WCAG 2.3.3][wcag-motion]
- **Flashing:** at most one bright flash per reveal, and never more than three per second. [WCAG 2.3.1][wcag-flash] A large card can approach WCAG's flash-area guide of 341 × 256 px, so keep any brightness jump small.
- Limit the effect to the card and its surroundings. Never shake the window. It is someone's desktop.

**Leave out, and why:**

| Leave out | Why |
| --- | --- |
| Reels, wheels, stop-next-to-rare | Near misses: disputed benefit, and the behaviour New York is suing over |
| Fake tells above true rarity | Deception for its own sake |
| Streak display, bonuses, loss warnings | Silverman & Barasch; they would increase drinking |
| Progress toward the next rare, pity timers | Goal gradient: speeds the next can |
| Rewards for completing a flavour or set | Pushes people toward more cans, and is unreachable anyway |
| Daily rewards, timers, notifications | Named dark patterns; no benefit here |
| Fanfare on Commons | The losses-disguised-as-wins pattern, and it would tire fastest |
| Effects from the first frame | Overwatch's finding that too much light kills anticipation |
| Reveals over about 5 s, or unskippable ones | EA's experience |
| TeamSpeak audio | Project rule |

**Implementation notes** (*from the code; not built or tested*):

1. Trigger on `Notice::Added` and render on the existing `large_card`.
2. Open a playback path that doesn't depend on a connected server, otherwise there is no sound when offline.
3. Warm the Fire, Electric and Distort pipelines when the page opens. `prepared_renderer` warms only the ring, and its comment warns that cold pipelines stall for hundreds of milliseconds. *Inferred:* the first Legendary, the worst possible moment, would be the one to hitch.
4. Skipping needs its own short fade buffer, since `play` replaces the current sound abruptly.
5. Add a `--snap` scene for each stage of each rarity. `AGENTS.md` requires it for UI changes.

**How to test the guess.** Build Common and Legendary first, then use it for two weeks. If Commons ever feel slow, cut Settle before anything else. If Legendaries stop feeling special, lengthen the Hush, not the build.

## What could not be established

- A published pack-reveal timing table or GDC talk on reveal pacing.
- Measured durations for Hearthstone, Pocket, Genshin or Counter-Strike reveals.
- Official Genshin colour meanings or skip behaviour, and Pocket's skip behaviour.
- A controlled study that set completion causes repeat buying.
- A preregistered large replication of the goal-gradient field result.
- Any measurement of collectibles or streaks changing energy-drink intake.
- Whether rising-pitch stingers work better than other reward sounds.
- The text of the European Parliament's resolution, and whether a Digital Fairness Act proposal had been published by this date.

## Sources

[schultz97]: https://pubmed.ncbi.nlm.nih.gov/9054347/
[schultz16]: https://pmc.ncbi.nlm.nih.gov/articles/PMC4826767/
[fiorillo-pdf]: https://www.pdn.cam.ac.uk/system/files/documents/2003-fiorillo-science.pdf
[niv]: https://pubmed.ncbi.nlm.nih.gov/15953384/
[hart]: https://pubmed.ncbi.nlm.nih.gov/25172480/
[berridge]: https://pubmed.ncbi.nlm.nih.gov/17072591/
[horsley]: https://pubmed.ncbi.nlm.nih.gov/22274620/
[haw]: https://epe.bac-lac.gc.ca/100/201/300/jrn_gambling_issues/html/2008/no22/issue21/04haw.htm
[larche]: https://pmc.ncbi.nlm.nih.gov/articles/PMC7882574/
[kocher]: https://econpapers.repec.org/RePEc:cpr:ceprdp:7715
[salimpoor]: https://pubmed.ncbi.nlm.nih.gov/21217764/
[ferreri]: https://pubmed.ncbi.nlm.nih.gov/30670642/
[clark]: https://pmc.ncbi.nlm.nih.gov/articles/PMC2658737/
[chase]: https://pubmed.ncbi.nlm.nih.gov/20445043/
[kassinove]: https://pubmed.ncbi.nlm.nih.gov/11419232/
[dixon13]: https://pubmed.ncbi.nlm.nih.gov/23054386/
[pisklak]: https://pubmed.ncbi.nlm.nih.gov/31522339/
[kivetz]: https://business.columbia.edu/sites/default/files-efs/pubfiles/1200/goalgradient.pdf
[bonezzi]: https://pubmed.ncbi.nlm.nih.gov/21474842/
[coglode]: https://www.coglode.com/nuggets/endowed-progress-effect
[evers]: https://pubmed.ncbi.nlm.nih.gov/23773159/
[monolith]: https://monolith.law/en/general-corporate/game-random-complete-illegal
[silverman]: https://udspace.udel.edu/items/42ce576b-8e1f-429a-8541-e29e48dbcfbb
[ruggeri]: https://pubmed.ncbi.nlm.nih.gov/32424259/
[duolingo]: https://blog.duolingo.com/how-duolingo-streak-builds-habit/
[benishek]: https://pubmed.ncbi.nlm.nih.gov/24750232/
[michie]: https://pubmed.ncbi.nlm.nih.gov/19916637/
[etkin]: https://marketing.wharton.upenn.edu/wp-content/uploads/2016/10/Etkin-Jordan-11-12-15-Hidden-Cost.pdf
[etkin-duke]: https://scholars.duke.edu/publication/1134328
[gdc-hs]: https://www.gdcvault.com/play/1022036/Hearthstone-How-to-Create-an
[gd-hs]: https://www.gamedeveloper.com/design/video-designing-an-immersive-user-interface-for-i-hearthstone-i-
[heiberg]: https://www.gamedeveloper.com/business/what-makes-a-great-in-game-loot-box-devs-weigh-in
[belgium]: https://www.skadden.com/-/media/files/publications/2019/08/video-gaming-egaming-law-update/fn12_onderzoeksrapportlootboxenengelspublicatie.pdf?la=en
[toucharcade]: https://toucharcade.com/2017/04/04/designing-hearthstone-card-packs-animations-iterations-ungoro-and-more-with-art-director-ben-thompson/
[hs-wiki]: https://hearthstone.wiki.gg/wiki/Open_Packs
[outof]: https://outof.games/news/5146-continuous-pack-opening-functionality-arrives-for-hearthstones-desktop-clients-with-the-new-patch-keep-your-spacebars-ready/
[nagaya]: https://play.google.com/store/apps/editorial?id=mc_games_editorialmd_pokemontcgpocket_interview_ryotsujikawa_and_satorunagaya_fcp
[brode]: https://marvelsnapzone.com/update-to-the-collection-level-road-ben-brode-explains-the-changes/
[ea-v11]: https://www.ea.com/games/ea-sports-fc/fc-27/news/update-v1-1-pitch-notes
[ea-preview]: https://www.ea.com/ea-play/news/pitch-notes-fifa21-preview-packs
[retrogems]: https://retrogems.fr/en/ea-sports-fc-27-fut-pack-animation-25-seconds-community-backlash/
[hostedgg]: https://hostedgg.com/blog/gacha-pull-reveal-color-culture
[ftc-gi]: https://www.ftc.gov/news-events/news/press-releases/2025/01/genshin-impact-game-developer-will-be-banned-selling-lootboxes-teens-under-16-without-parental
[nyag]: https://ag.ny.gov/sites/default/files/court-filings/new-york-v-valve-corporation-complaint-2026.pdf
[nyag-pr]: https://ag.ny.gov/press-release/2026/attorney-general-james-sues-game-developer-promoting-illegal-gambling-through
[swink]: https://www.gamedeveloper.com/design/game-feel-the-secret-ingredient
[juice]: https://gdcvault.com/play/1016487/Juice-It-or-Lose
[screenshake]: https://www.youtube.com/watch?v=AJdEqssNZ-U
[vlambeer]: https://gamedeveloper.com/design/vlambeer-co-founder-shares-advice-on-building-better-action-games
[gamefeel-survey]: https://ar5iv.arxiv.org/html/2011.09201
[nng]: https://www.nngroup.com/articles/response-times-3-important-limits/
[dixon14]: https://pubmed.ncbi.nlm.nih.gov/23821220/
[dixon10]: https://pubmed.ncbi.nlm.nih.gov/20712818/
[barrus]: https://pmc.ncbi.nlm.nih.gov/articles/PMC6602008/
[cherkasova]: https://pmc.ncbi.nlm.nih.gov/articles/PMC6596211/
[ccrma]: https://ccrma.stanford.edu/guides/planetccrma/Additive_Synthesis_on.html
[wcag-audio]: https://www.w3.org/WAI/WCAG22/Understanding/audio-control.html
[wcag-motion]: https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html
[wcag-flash]: https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html
[zendle18]: https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0206767
[zendle19]: https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0213194
[zendle21]: https://pubmed.ncbi.nlm.nih.gov/33822788/
[spicer]: https://www.open-access.bcu.ac.uk/14466
[garea]: https://ideas.repec.org/a/taf/intgms/v21y2021i3p460-479.html
[king]: https://pubmed.ncbi.nlm.nih.gov/29952052/
[uk2022]: https://www.gov.uk/government/consultations/loot-boxes-in-video-games-call-for-evidence/outcome/government-response-to-the-call-for-evidence-on-loot-boxes-in-video-games
[uk-principles]: https://www.gov.uk/guidance/loot-boxes-in-video-games-update-on-improvements-to-industry-led-protections
[clifford-nl]: https://www.cliffordchance.com/insights/resources/blogs/talking-tech/en/articles/2022/09/the-ultimate-loot-drop-the-netherlands-is-planning-to-ban-loot.html
[covington]: https://www.covingtonblogs.com/2023/02/16/upcoming-eu-legislation-on-loot-boxes/
[ep2023]: https://www.europarl.europa.eu/doceo/document/TA-9-2023-0008_EN.html
[cpc]: https://commission.europa.eu/document/download/75ff33d4-77a6-4f14-916a-766a8c6c281c_en?filename=CPC+Network+Joint+Statement+of+the+Consumer+Protection+Cooperation+Network+on+online+games.pdf
[lewis]: https://www.lewissilkin.com/insights/2026/10/15/eu-consumer-protection-network-launches-action-involving-nine-video-games-compani-102o5do
[zagal]: https://www.deceptive.design/articles/dark-patterns-in-the-design-of-games
[zagal-mock]: https://arxiv.org/html/2412.05039v1
[ftc-dp]: https://www.ftc.gov/reports/bringing-dark-patterns-light
[mathur]: https://arxiv.org/abs/1907.07032
[efsa-topic]: https://www.efsa.europa.eu/en/topics/topic/caffeine
[efsa-brochure]: https://www.efsa.europa.eu/sites/default/files/corporate_publications/files/efsaexplainscaffeine150527.pdf
[efsa-doi]: https://efsa.onlinelibrary.wiley.com/doi/10.2903/j.efsa.2015.4102
[fda]: https://www.fda.gov/consumers/consumer-updates/spilling-beans-how-much-caffeine-too-much
[drake]: https://pubmed.ncbi.nlm.nih.gov/24235903/
[juliano]: https://pubmed.ncbi.nlm.nih.gov/15448977/
[untappd]: https://arxiv.org/abs/2601.04841v1
