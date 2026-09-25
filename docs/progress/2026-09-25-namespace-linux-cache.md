# Linux cache comparison

Status: Namespace is connected to this repository. Running the full Linux comparison.
Source: kukuri-app/kukuri `6f506d4ead06652dbbe7411635bcf2c9ab8efa16`.
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
  which includes media-observer and database-close fixes. `3fa85b53` remains
  available for a subsequent real commit build.
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
