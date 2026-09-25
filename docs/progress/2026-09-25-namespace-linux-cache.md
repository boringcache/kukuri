# Linux cache comparison

Status: Completed the full mixed Linux cold/warm pair and next-commit build on the existing trial.
Cold/warm source: kukuri-app/kukuri `6f506d4ead06652dbbe7411635bcf2c9ab8efa16`.
Current rolling source: `3fa85b5344d540219b937555bd8a673fc5919d92`.
Branch: `linux-cache-trial` in `boringcache/kukuri`.

The first experiment compares BoringCache directory caches with Namespace Rust
cache volumes. It preserves the pinned upstream Linux commands, toolchains,
timeouts, artifact paths, pruning, and build settings. Windows is excluded.
The existing GitHub OIDC connection authenticates BoringCache.

Acceptance criteria:

- AC-1: The BoringCache variant runs desktop UI, smoke, and community-node on
  public GitHub Ubuntu 24.04 runners (4 vCPU / 16 GB), and the other five Linux
  jobs on Namespace Ubuntu 24.04 (8 vCPU / 16 GB) without cache volumes.
- AC-2: The Namespace baseline uses Namespace 4 vCPU / 8 GB for the three small
  jobs and 8 vCPU / 16 GB for the five large jobs, with explicit 20 GB cache
  volumes. The small-job comparison therefore changes hardware and cache;
  the large-job comparison holds the machine size constant.
- AC-3: A cold run publishes directory caches; a separate warm run of the same
  commit requires restored cache state on fresh runners. Report setup, restore,
  build/test, save, job duration, queue time, and available storage evidence.
  Failed or missing-cache samples are not performance wins. The initial base `ff491fa1` failed browser and backup tests under both
  cache providers. The trial now uses the next real upstream commit `6f506d4e`,
  which includes media-observer and database-close fixes. The subsequent real
  commit build at `3fa85b53` is recorded below.
- INVAR-1: Keep all eight upstream Linux job workloads and omit Windows.
- INVAR-2: Do not change signed-release workflows, upstream code, account plans,
  or existing Namespace profiles.

Verification: actionlint, CLI dry-run profile resolution, comparison of the
preserved upstream steps, then the selected cold/warm GitHub Actions runs.
No product behavior or authentication policy is changed; this is a fork-local
workflow experiment using the existing cache connection.

Dispatch `Kukuri Linux cache comparison` (`kukuri-fast.yml`) on this branch:

- `provider`: `boringcache` or `namespace`.
- `phase`: `cold` seeds, `warm` restores without publishing for BoringCache,
  and `commit` requires a hit and publishes the real source change.
- `cli_version`: optional BoringCache CLI canary override; normally empty.

Cache identities are separate from the earlier validation. Native Namespace
tags are also separate from Kukuri's upstream tags. Every run includes all eight Linux jobs. Run cold once, then warm at the exact
same commit in this workflow stream. No sccache layer is added
in this first directory-cache comparison. The upstream pnpm cache setting is
preserved for both providers. Report any difference in the available pnpm cache
state; it is outside the Rust directory-cache comparison.

Namespace Cache Volumes must be absent from BoringCache runners. A job checks
the cache-path environment before configuring BoringCache and records runner
CPU, memory, filesystem mounts, and disk usage. The baseline's 20 GB requested
capacity is explicit; Kukuri's current private profile capacities are unknown.
Cold Namespace jobs may reuse a tag populated by another job in the same run,
just as the upstream desktop and harness jobs share their tags.

The trial has a 32-vCPU Linux concurrency limit. Separate queue time from job
duration; the baseline's full workflow cannot reproduce Kukuri's Team capacity.
Namespace enrollment was approved and completed with only this repository
selected. The earlier three-job GitHub run is a pilot, excluded from the final
comparison. The full BoringCache profile uses new tags so that pilot cannot
prewarm its cold run. Run labels identify the cache provider and phase; job
labels identify the actual runner provider.

Cargo registry, Git dependency, and target tags follow the same six workload
groups as Namespace volumes. Desktop UI/browser share one group; smoke and
community-node share one group. Other workloads have separate tags, so an
unrelated job cannot replace their dependency directories during a parallel save.
The initial full run was cancelled after this configuration issue was found;
the replacement cold run uses fresh tags.

The corrected cold run at `ff491fa1` passed six jobs; browser and Rust tests
failed in upstream tests under both providers. Rust test caches were not saved,
so no complete warm claim is possible at that base. After advancing to
`6f506d4e`, both providers use new cache tags for a fresh cold/warm pair.

## Completed runs

The [rolling commit build](https://github.com/boringcache/kukuri/actions/runs/36160851759)
at experiment merge `0729b77fc38f1342197530e17e8204b9d2541dd8` advanced exactly one
upstream first-parent commit to `3fa85b53`. It reused the existing cache tags,
enabled publication, and passed all eight jobs. Each job reported a cache hit,
restored three entries, and saved three entries. No new Namespace-volume run
was launched.

Against the [upstream run at that source](https://github.com/kukuri-app/kukuri/actions/runs/36148214345),
our Linux completion was 9m20s versus 7m30s. Upstream's Windows-inclusive total
was 7m55s. Our desktop UI job determined completion at 9m15s; its Vitest step
took 7m15s versus 5m53s upstream, while Rust cache setup took 5s versus 2s.
Both passed the same 254 files and 2,056 tests. This supports runner performance
as the main explanation for that job's slowdown, without isolating CPU speed,
contention, or other environment effects.

Estimated Namespace unit-minutes were 180.40 versus 221.20 upstream (18.4%
lower). The five jobs staying on Namespace took 22m33s combined versus 21m42s
(3.9% longer), including our cache saves. The lower usage comes from moving
three jobs to GitHub. At $0.0015 per overage unit-minute, the marginal compute
difference is about $0.0612 per equivalent run; actual invoice savings depend
on allowances, billing overhead, storage, BoringCache charges, and Windows.

The following cold/warm results predate that rolling build.

Both mixed runs used experiment commit `33d9434d5e26bb3d23cc93bde5134364a40ada0b`
and upstream source `6f506d4e`. Each run contains all eight Linux jobs: three on
GitHub and five on Namespace without cache volumes.

- [Mixed cold](https://github.com/boringcache/kukuri/actions/runs/36154130003): eight passed. Seven jobs missed their fresh cache tags; the later browser job restored the desktop cache saved by UI in the same run.
- [Mixed warm](https://github.com/boringcache/kukuri/actions/runs/36156611654): eight passed, all eight restored three entries, and the Action skipped saves under `trust-policy: restore`.
- [Native cold](https://github.com/boringcache/kukuri/actions/runs/36154392813): eight passed.
- [Native warm attempt](https://github.com/boringcache/kukuri/actions/runs/36156685034): desktop UI passed with a hit; seven jobs failed the strict cache-hit check before running their workload. These are excluded from warm performance totals. Namespace documents cache misses while new tags spread across its fleet.

The [upstream run at the same source](https://github.com/kukuri-app/kukuri/actions/runs/36143052712)
passed all eight Linux jobs with native cache hits. Using its documented runner
sizes, it consumed approximately 203.87 Namespace CPU-minutes versus 176.80 for
our mixed warm profile, 13.3% less Namespace compute exposure. This comes from
moving three jobs to GitHub. The five Namespace jobs took 22.10 job-minutes
versus 20.48 upstream, 7.9% longer. This sample does not establish a speed win or
net invoice savings. It excludes allowances, provisioning overhead, BoringCache
charges, normal source-changing save work, and Windows. Queue delays are not
execution time; the trial has 32 Linux CPUs and the two providers overlapped.

The new native tags request 20 GB each across six groups (120 GB), with about
21.1 GB occupied in populated snapshots. Five older baseline tags remain retained
separately. BoringCache runners passed the no-volume check; an inspected Namespace
instance explicitly reported no attached volumes and 96 GB ephemeral storage.
Two duplicate idle experiment runners were stopped after verifying they had no
job and that the actual jobs were running on other instances.

Warm authentication was restore-only at the Action policy level. Broker
diagnostics still advertised stage/save capability, so this does not prove a
credential restricted to reads. No plan upgrade or Windows run was performed.
The next real upstream commit `3fa85b53` remains available for a commit run.
