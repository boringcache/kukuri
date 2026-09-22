# Issue #1292 live・game 一覧の channel 別上限

## 対象

- Issue: #1292
- Scope revision: `2026-09-22-r1`
- 基準 commit: `70574678582d0ce79760819866f3cce63fa3165e`
- リスク区分: C
- 状態: 実装・ローカル検証完了（PR / 監査 / merge の状態はIssueとPRを正本とする）

## 変更前の再現

`cargo test -p kukuri-store live_and_game_lists_are_channel_bounded_index_reads -- --nocapture` を修正前SQLに対して実行し、失敗を確認した。

```text
live sessions must use the channel-bounded index range:
SEARCH live_session_cache USING COVERING INDEX idx_live_session_cache_topic_started_all (topic_id=?)
```

現行のlive一覧は `topic_id` だけの索引範囲を読み、channel条件を持たなかった。game一覧も同じSQL形状だった。

## 実装

- storeの一覧契約を、topic・channel・limitが必須の `list_channel_live_sessions` / `list_channel_game_rooms` に置き換えた。
- SQLiteは本番とquery plan testで同じSQL builderを使い、既存のchannel別複合索引から固定件数を読む。
- MemoryStoreは `(topic_id, channel_id)` ごとの時刻・ID降順secondary indexを持ち、upsertによる時刻・channel変更とprojection再構築時に同期する。
- app-apiの初回取得とcatch-up後の再取得はどちらも100行を上限とする。
- 初回取得と条件付き再取得は共通helperを通し、store一覧の呼出回数を通常1回、refresh時2回、refresh失敗時1回に固定する。
- 参加中liveのheartbeatは対象sessionを単一行で確認し、一覧窓より古いsessionでも終了projectionを観測したら自身のtaskを停止する。
- ScoreGameの反映済みcache比較は、一覧窓外でも対象を失わない `get_game_room` の単一行取得へ移した。
- 旧topic全件一覧APIと、app-api側の事後channel filterを削除した。

## AC / INVAR evidence

| 条件 | 実装・test |
| --- | --- |
| AC-1 / AC-2 / AC-5 | `sqlite/live_game.rs` の共通SQL builder、query plan / backend parity、`projection_list_calls_the_store_once_or_twice_only` |
| AC-3 / INVAR-3 | `get_game_room`、`game_projection_freshness` の15 test |
| AC-4 / INVAR-2 / TR-6 | MemoryStore secondary index、`live_and_game_lists_are_bounded_and_channel_indexed_in_both_backends` |
| INVAR-1 / TR-3 / TR-4 | app-apiのlive、Dome listing、session catch-up、session integrityのtargeted test |
| INVAR-4 | trait callerの全移行、旧全件APIの参照0件、既存IPC view型は変更なし |

## Validation

- 修正前再現: `cargo test -p kukuri-store live_and_game_lists_are_channel_bounded_index_reads -- --nocapture` — FAIL（topicだけの索引範囲）
- 修正後の同test — PASS
- targeted store / app-api tests — PASS
  - SQLite / MemoryStoreのchannel・limit・更新・単一行取得
  - live、Dome listing、game projection freshness、session catch-up、session integrity、hint rehydration
  - 一覧窓外の終了live heartbeat停止
  - 一覧store呼出回数は通常1回、refresh時2回、refresh失敗時は再取得なし
- `cargo xtask rust-test` — PASS（最新main取込・最終delta後は1,352 passed、5 skipped）
- `cargo xtask scenario desktop_smoke_live_session_persist` — PASS（8 steps）
- `cargo xtask scenario desktop_smoke_game_room_persist` — PASS（7 steps）
- `cargo xtask check` — PASS
  - 初回はfrontend依存未導入のためeslintを起動できず失敗した。
  - `npx pnpm@10.16.1 install --dir apps/desktop`後に再実行し、fmt、clippy、Tauri check、frontend lint・typecheckが成功した。
- `git diff --check` — PASS
- 最新mainの取込後、CIで `instance_lookup_reads_a_constant_number_of_docs_records` が失敗した。同期lookupのcounterへcreate時に起動したsession catch-upの読み出しが並行して混ざるtest競合で、製品のlookup結果や上限の失敗ではなかった。計測前に購読taskを停止して同期lookupだけを測るようにし、単独10回連続と全suiteで成功した。

PR head、独立監査、CI、merge後tree確認はIssueとPRへ記録する。

## 追加発見

- `clear_expired_live_presence` は期限切れpresenceの全channel削除、liveのviewer countはsession内presence件数の集計であり、一覧projectionの行数とは別の比例経路である。Issue #1292の固定範囲外のため、この作業では変更しない。
