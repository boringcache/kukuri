# BoringCache validation for issue #1073

Upstream: [kukuri-app/kukuri#1073](https://github.com/kukuri-app/kukuri/issues/1073).
Fork: `boringcache/kukuri`, branch `boringcache-validation`.
Workspace: `boringcache/kukuri`, GitHub OIDC, publication mode `verified_context`.
No repository secrets.

The change is cache-only. Runners, toolchains, job matrix and build commands are
upstream's. `Kukuri Fast` keeps its eight Linux jobs and `windows-fast`.

## Sources

| Case | Run | Source |
| --- | --- | --- |
| cold | [35223130289 attempt 1](https://github.com/boringcache/kukuri/actions/runs/35223130289/attempts/1) | `75acc64b13b0` |
| warm | [35223130289 attempt 2](https://github.com/boringcache/kukuri/actions/runs/35223130289) | `75acc64b13b0` |
| rolling 1 | [35229995862](https://github.com/boringcache/kukuri/actions/runs/35229995862) | `b65e4a48d4bd` |
| rolling 2 | [35235858484](https://github.com/boringcache/kukuri/actions/runs/35235858484) | `351249d3dd4c` |

Cold and warm are the same commit on two attempts of one run. Rolling 1 and 2
are the two following upstream `main` revisions.

## Job time

| Job | Cold | Warm | Rolling 1 | Rolling 2 | Cold → warm |
| --- | ---: | ---: | ---: | ---: | ---: |
| linux-cn | 20m00s | 11m33s | 16m35s | 11m47s | −42.2% |
| linux-cn-e2e | 9m31s | 4m52s | 5m11s | 5m41s | −48.9% |
| linux-community-node | 6m53s | 2m44s | 2m51s | 2m33s | −60.3% |
| linux-desktop-browser | 21m14s | 17m27s | 16m05s | 17m08s | −17.8% |
| linux-desktop-ui | 13m22s | 12m44s | 11m09s | 11m19s | −4.7% |
| linux-rust-static | 17m20s | 7m05s | 6m30s | 6m12s | −59.1% |
| linux-rust-tests | 21m49s | 16m21s | 17m54s | 19m27s | −25.1% |
| linux-smoke | 5m45s | 2m20s | 2m54s | 1m57s | −59.4% |
| windows-fast | 44m46s | 16m50s | 18m37s | 12m35s | −62.4% |
| **Total runner time** | **160m40s** | **91m56s** | **97m46s** | **88m39s** | **−42.8%** |
| Wall time (slowest job) | 44m46s | 17m27s | 18m37s | 19m27s | |

Job time includes checkout, toolchain install and test execution, not queueing.

## sccache

Executed compile requests, hits, and hit rate from each job's `Show sccache
stats` step.

| Job | Cold | Warm | Rolling 1 | Rolling 2 |
| --- | --- | --- | --- | --- |
| linux-cn | 495/2324 (21.34%) | 240/240 (100.00%) | 240/240 (100.00%) | 240/240 (100.00%) |
| linux-cn-e2e | 67/1324 (5.08%) | 239/239 (100.00%) | 239/239 (100.00%) | 239/239 (100.00%) |
| linux-community-node | 3/1023 (0.29%) | 25/25 (100.00%) | 23/25 (92.00%) | 25/25 (100.00%) |
| linux-desktop-browser | 20/1023 (1.96%) | 25/25 (100.00%) | 23/25 (92.00%) | 25/25 (100.00%) |
| linux-desktop-ui | 3/1023 (0.29%) | 25/25 (100.00%) | 25/25 (100.00%) | 25/25 (100.00%) |
| linux-rust-static | 615/3551 (17.36%) | 257/257 (100.00%) | 255/257 (99.22%) | 257/257 (100.00%) |
| linux-rust-tests | 112/1180 (9.52%) | 259/259 (100.00%) | 255/259 (98.46%) | 259/259 (100.00%) |
| linux-smoke | 3/1023 (0.29%) | 25/25 (100.00%) | 25/25 (100.00%) | 25/25 (100.00%) |
| windows-fast | 0/2099 (0.00%) | 47/47 (100.00%) | 43/47 (91.49%) | 47/47 (100.00%) |

Cache read errors: 0 in every job of every case.
Cache write errors: 0 in every job of every case.

Executed request counts fall between cold and warm because Cargo restores its
own target directories from BoringCache and does not re-invoke rustc for units
it does not need to rebuild. The percentages above are sccache's own rate over
the requests each job executed.

Issue #1073 reports sccache at 5–51% with 535–3093 write errors per job and a
`windows-fast` job of about 48 minutes against a 10 GB Actions cache allowance
holding 14.5–19.4 GB. The cold case above reproduces that hit-rate range and
`windows-fast` duration on this fork. Every warm and rolling job reports zero
write errors.

## Cache layout

`.boringcache.toml` declares one sccache object store shared by all nine jobs
(`kukuri-fast-rust`), one shared Cargo registry/index/git-db set, and one target
entry per job, keeping the separation the upstream shared-key scheme had. None
of it is charged to the GitHub Actions allowance.

## Scope

This branch also carries scaffolding that is not part of the cache migration:
`boringcache-connect.yml`, the validation branch added to `kukuri-fast.yml`'s
push trigger, and a `Show sccache stats` step on `windows-fast`.

Not measured: merge-queue concurrency, fork pull-request access, and any job
outside `Kukuri Fast`.
