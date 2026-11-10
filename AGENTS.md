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
