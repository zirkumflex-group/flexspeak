# Runs before every job (ACTIONS_RUNNER_HOOK_JOB_STARTED). The workflows only send trusted
# jobs here, but a pull request can edit its own workflow, so this is the check that counts:
# a refused job fails before any of its code runs.
$allowed = @('yungtristxn', 'Jan-GJ')
$repository = 'zirkumflex-group/flexspeak'
function Refuse($why) {
    Write-Output "::error::The Beelink runner only takes $repository jobs started by $($allowed -join ' or '): $why"
    exit 1
}
if ($env:GITHUB_REPOSITORY -ne $repository) { Refuse "this job is from $env:GITHUB_REPOSITORY" }
if ($env:GITHUB_EVENT_NAME -notin @('push', 'pull_request', 'workflow_dispatch')) {
    Refuse "the $env:GITHUB_EVENT_NAME event is not allowed"
}
foreach ($who in @($env:GITHUB_ACTOR, $env:GITHUB_TRIGGERING_ACTOR)) {
    if ($allowed -notcontains $who) { Refuse "this job was started by '$who'" }
}
$event = Get-Content -Raw $env:GITHUB_EVENT_PATH | ConvertFrom-Json
if ($env:GITHUB_EVENT_NAME -eq 'pull_request' -and $event.pull_request.head.repo.full_name -ne $repository) {
    Refuse "pull requests from forks run on GitHub's runners"
}
Write-Output "Trusted job: $env:GITHUB_EVENT_NAME by $env:GITHUB_ACTOR"
