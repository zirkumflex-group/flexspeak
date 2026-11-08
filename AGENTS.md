# Agent notes

## UI changes

When a change adds, removes or alters visible UI, show the result inline in the thread before finishing: render every affected screen and state with `flexspeak --snap <file>.png --scene <name>` (`--list-scenes` names them, and a new state gets a new scene), then embed each PNG in your reply with Markdown image syntax and an absolute path. Done means each changed state is visible in the thread, including one that was removed from the UI.

## Changelog

Settings → Updates and the GitHub release notes are built by `scripts/release.py` from the message of the commit that lands on `main`. A change people can notice needs an entry there, or the app shows only the commit subject under Changed.

- In the PR description, write one line per change starting `New:`, `Fixed:` or `Changed:`, in plain language for people using the app.
- When merging, squash and pass those lines as the commit body, so they reach `main`: `gh pr merge <number> --squash --body-file <notes>`. A merge commit drops them.
- Don't add build entries to `CHANGELOG.md` by hand; the release build adds them. Edit it only to reword an entry.
