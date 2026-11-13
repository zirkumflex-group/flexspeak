# Agent notes

## UI changes

When a change adds, removes or alters visible UI, show the result inline in the thread before finishing: render every affected screen and state with `flexspeak --snap <file>.png --scene <name>` (`--list-scenes` names them, and a new state gets a new scene), then embed each PNG in your reply with Markdown image syntax and an absolute path. Done means each changed state is visible in the thread, including one that was removed from the UI.

The PR carries the same screenshots, in a `## Screenshots` section of its description: one image per changed state, captioned with the scene name, plus a before shot (rendered from `main`) for a state that changed or was removed. Push the PNGs to the `pr-screenshots` branch in a folder named after the PR branch, never to the PR branch itself, so they stay out of `main`:

```sh
wt=$(mktemp -d) && git worktree add --detach "$wt" && cd "$wt"
git fetch origin pr-screenshots && git switch pr-screenshots || git switch --orphan pr-screenshots
mkdir -p "<pr-branch>" && cp /path/to/*.png "<pr-branch>/"
git add . && git commit -m "Screenshots for <pr-branch>" && git push origin pr-screenshots
cd - && git worktree remove "$wt"
```

Embed each by its raw URL: `![<scene>](https://raw.githubusercontent.com/zirkumflex-group/flexspeak/pr-screenshots/<pr-branch>/<file>.png)`. When the UI changes again during review, push the new shots under new file names, since GitHub caches images by URL, and update the section.

## Changelog

Settings → Updates and the GitHub release notes are built by `scripts/release.py` from the message of the commit that lands on `main`. A change people can notice needs an entry there, or the app shows only the commit subject under Changed.

- In the PR description, write one line per change starting `New:`, `Fixed:` or `Changed:`, in plain language for people using the app.
- When merging, squash and pass those lines as the commit body, so they reach `main`: `gh pr merge <number> --squash --body-file <notes>`. A merge commit drops them.
- Don't add build entries to `CHANGELOG.md` by hand; the release build adds them. Edit it only to reword an entry.

## Logs

Every run of the client writes a log, kept on the user's computer (`src/diagnostics.rs`). Read it before guessing at what happened.

- Where: `%LOCALAPPDATA%\Venator\TeamSpeakNative\data\logs` on Windows; Settings → Diagnostics → Open Folder shows it. `NATIVE_VOICE_LOGS` moves it, for test runs.
- Files: `flexspeak-<YYYYMMDD-HHMMSS>-<part>.jsonl` for the running app, gzipped to `.jsonl.gz` once that run is over. Name order is time order. Read a gzipped one with `gzip -dc <file> | jq …` (or Python's `gzip` module).
- Lines: one JSON object each, keys `ts` (local time with offset), `level`, `target`, `src` (file:line), `thread`, optional `spans`, then `msg` and the event's own fields. A field named like one of those is written with a `_` in front.
- The first line of a run is `msg: "session start"` (version, build, OS, CPU, args, whether debug logging is on); a clean exit ends with `"session end"`. A log without the end line crashed or was killed; look for `target: "flexspeak::crash"` (a panic with backtrace, or a native crash with module and offset).
- Useful targets: `flexspeak::alerts` (every problem the app showed, and its recovery), `flexspeak::connection` (status, stops and why, usage), `flexspeak::stream` (shares, watches, per-stream stats), `flexspeak::update`, `flexspeak::ui` (what was clicked, with debug logging on).
- Debug logging (Settings → Diagnostics, `debug_logging` in settings.json) adds debug detail and the voice libraries' info; `RUST_LOG` overrides both levels. Save Diagnostics zips recent logs, `settings.json` with secrets removed, `state.json` and the Error Log into Downloads.
- When adding logging: put user-typed text, chat and secrets in no field; make per-frame or per-packet events `trace`, not `debug`.
