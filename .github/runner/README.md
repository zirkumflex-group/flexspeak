# Beelink Windows runner

Windows checks and release builds can run on a Windows Server 2022 VM on the Beelink
(`bee`, `/srv/flexspeak-ci`) instead of GitHub's `windows-2022` runner. It keeps Cargo's
output between jobs, so a run compiles only what changed instead of restoring a cache first.

## Switching

The repo variable `WINDOWS_RUNNER` (Settings → Secrets and variables → Actions →
Variables) names the runner label, `flexspeak-win`. The next run uses it:

```sh
gh variable set WINDOWS_RUNNER --body flexspeak-win   # Beelink
gh variable delete WINDOWS_RUNNER                     # back to GitHub's runner
```

Jobs wait in the queue while the Beelink is down; delete the variable to move them back.
GitHub's runner still restores its caches when it takes over, though they are as old as
the last run it made.

## Who it runs for

Only `yungtristxn` and `Jan-GJ`, and never a pull request from a fork. The workflows send
every other run to GitHub's runner (`FLEXSPEAK_WINDOWS_RUNNER` in their `platforms` job).
A pull request can edit its own workflow, so the runner checks again before every job
(`oem/allow-job.ps1`, a job-started hook) and fails one it doesn't trust before any of its
code runs. To change who it trusts, edit both lists and copy the hook into the VM
(`C:\ci\allow-job.ps1`).

## The VM

[dockur/windows](https://github.com/dockur/windows) runs it as a container with KVM:
12 cores at a quarter of the CPU weight of anything else on the box, 12 GB of RAM, a disk
of up to 128 GB. It reaches the internet and nothing else: `firewall.sh`
(`flexspeak-ci-firewall.service`) drops what it sends to private networks and to the
Beelink itself. Its ports are bound to the Beelink's localhost.

| Files on `bee` | |
|---|---|
| `compose.yaml` | the container; the Windows password is in `.env` |
| `oem/` | copied to `C:\OEM` on install: `install.bat` turns on SSH, `setup.ps1` installs the tools and runners |
| `vm` | runs a command in the VM over SSH: `/srv/flexspeak-ci/vm hostname` |
| `storage/` | the VM's disk |

Two runners, `beelink-1` and `beelink-2`, run as services under the VM's user `ci`.
Build output lives in `C:\ci\target\checks` (the checks and the release's test) and
`C:\ci\target\release`, and Opus in `C:\ci\opus`; deleting them makes the next run start
from scratch. Defender leaves those, the checkouts and the toolchain alone (`setup.ps1`).

The web console is at `ssh -L 8006:127.0.0.1:8006 bee`, then http://127.0.0.1:8006.

### Rebuilding it

```sh
cd /srv/flexspeak-ci && docker compose down && rm -rf storage/*   # on bee
docker compose up -d                                              # installs Windows (~20 min)
./vm "powershell -ExecutionPolicy Bypass -File C:\OEM\setup.ps1 -Token <token> -Password <ci's password>"
```

The token comes from `gh api -X POST repos/zirkumflex-group/flexspeak/actions/runners/registration-token -q .token`
and lasts an hour. Remove the old runners under Settings → Actions → Runners first.

### Licence

It runs the free Windows Server evaluation, which lasts 180 days. `slmgr /dlv` shows the
days left; `slmgr /rearm` and a restart add another 180 days, up to five times. After
that, rebuild it as above.
