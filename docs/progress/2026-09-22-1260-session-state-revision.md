# #1260 live session・ScoreGame の署名済み revision（2026-09-22）

- 対象 Issue: #1260（区分 C、Scope revision `2026-09-22-revision-only`、基準 commit `59900e54`）。
- Goal: owner が過去に署名した live session / ScoreGame の state を置き直しても、表示と操作の土台が過去へ戻らない。
- 仕様: [ADR 0052](../adr/0052-scale-independent-timeline-sync.md) §2、[ADR 0005](../adr/0005-live-session-data-classification.md)、[ADR 0006](../adr/0006-game-room-data-classification.md)。

## Scope

live session と ScoreGame は本番 record が無いため、旧形式の移行・後方互換は行わない。owner の署名対象となる revision を必須にし、revision の無い旧形式は拒否する。
Metaverse room は ScoreGame の revision 対象外で、既存の authority、書き込み、復元、移動、削除を維持する。

## 修正前の再現

`crates/app-api/src/tests/sync/hydration_integrity_sessions_contract.rs` に次の test を先に追加し、基準実装で失敗することを確認した。

- `replayed_older_live_session_does_not_revive_an_ended_session`: 期待 `Ended`、実際 `Live`。
- `replayed_older_score_game_does_not_restore_an_old_score`: 期待 `Running`、実際 `Waiting`（score も更新前へ戻る経路）。

いずれも、owner が作成時に署名した envelope と state を、更新後に同じ state key へ置き直し、個別反映を発火した。

## 変更

- `LiveSessionManifestBlobV1.revision`: 必須の `i64`。初版 1、終了時に checked increment。
- `GameRoomManifestBlobV1.score_revision`: ScoreGame では必須の `Some(i64)`。初版 1、score/status 更新時に checked increment。Metaverse は `None`。
- `load_verified_live_session` / `load_verified_game_room`: 同じ key の検証済み候補を署名済み revision で選ぶ。unsigned な `state.updated_at` を live / ScoreGame の新旧判定に使わない。
- projection: live の `revision`、game の `score_revision` を Memory / SQLite に保存する。upsert は、既存の ScoreGame / live の revision 以下を原子的に無視する。Metaverse の `None` は従来どおり更新する。
- 操作読み出し: docs の候補が永続 projection より古ければ返さない。古い状態を土台に owner が次の状態へ署名する経路を閉じる。
- schema migration `20260922010000_session_manifest_revisions`: live / game projection に revision 列を追加する。payload の旧形式を変換する data migration は行わない。

## inventory と状態遷移

- 個別反映: docs event、hint、key 指定、retry は `load_verified_live_session` / `hydrate_game_room_from_key` と revision guard を通る。
- 上限つき追いつき: `catch_up_sessions` は record をどの順序で読んでも、永続 upsert guard により最大 revision から戻らない。
- 操作: `fetch_live_session_state_and_manifest` / `fetch_game_room_state_and_manifest` の全 caller は同じ候補選択を使い、projection より古い候補を受け取らない。
- 自分の書き込み: live の作成・終了、ScoreGame の作成・更新が revision を採番する。ScoreGame の room 単位 lock と canonical comparison 後の commit 順序は維持した。
- projection sink: `upsert_live_session_cache` / `upsert_game_room_cache` の Memory / SQLite 実装で比較と mutation を同じ排他・SQL statement 内に置いた。
- Metaverse: `score_revision = None` のまま従来の upsert を行う。既存の作成・chat・customization・asset・move・delete・hosting caller は ScoreGame revision guard の対象外。

逆引きは `upsert_live_session_cache|upsert_game_room_cache|fetch_live_session_state_and_manifest|fetch_game_room_state_and_manifest|persist_live_session_manifest|persist_game_room_manifest|load_verified_live_session|load_verified_game_room|catch_up_sessions` を `crates/` で検索した。Issue の fixed inventory に対する未分類は 0。

## test 対応

- AC-1: replay contract を event、hint、key 指定、`catch_up_sessions` で実行し、live の status と ScoreGame の status / score が維持されることを確認する。
- AC-2 / AC-3: `*_selection_uses_signed_revision` で、古い候補の unsigned `updated_at` を `i64::MAX` にしても revision 2 を選ぶ。projection より古い docs state は操作へ返さない。
- AC-4: 読み出しは既存の `MAX_ENVELOPE_RECORDS_PER_OBJECT` と session window を維持し、全件取得を追加していない。既存の scale / catch-up test で確認する。
- AC-5: ADR 0052 / 0005 / 0006 を更新した。
- INVAR-1: 初版 1、同じ秒でも更新は revision 2。overflow は書き込み前に error とする。
- INVAR-2 / INVAR-3: `hydration_integrity_sessions*` と `game_projection_freshness` を維持する。
- INVAR-4: app-api の Metaverse test 群と store の `session_revision_guards_match_between_backends` で `None` の更新を確認する。
- 旧形式対象外: `session_manifests_without_revision_are_rejected`。

## ローカル検証

- 修正前再現: 上記 replay test 2 件が失敗。
- targeted: `hydration_integrity_sessions` 24 件、`game_projection_freshness` 15 件、revision selection 2 件、旧形式拒否 1 件が成功。
- `cargo test -p kukuri-app-api --lib`: 425 件成功。
- `cargo test -p kukuri-store --lib`: 123 件成功。追加後の `session_revision_guards_match_between_backends` も Memory / SQLite の両方で成功。
- `cargo xtask check`: Rust workspace の format / clippy、Tauri backend compile、desktop lint / typecheck が成功。初回は `node_modules` が無く frontend で停止したため、runbook の `pnpm install --frozen-lockfile` 後に再実行した。
- `cargo xtask app-api-slow-test`: iroh integration を含む 455 件成功。
- `cargo xtask scenario desktop_smoke_live_session_persist`: 8 steps 成功。`desktop_smoke_game_room_persist`: 7 steps 成功。
- `cargo xtask oversized-files`: 成功。revision の event / hint / catch-up / fresh viewer / operation の一連の contract が既存の shared fake と helper を使うため、test を分断・重複させず同じ contract file に置き、`xtask/oversized-baseline.json` に 1,206 行で登録した。production file の新規 oversized は無い。
- `cargo xtask test`: 対象 test を含む 1,043 件までは成功した後、既存 CLI Dome test の一時的な `committed` / `no_op` 差で fail-fast。単独再実行は成功した。再実行では既存 Iroh private-channel test が actor panic、単独再実行は成功した。`--no-fail-fast` の全 1,358 件では 1,350 件成功し、Windows の同時実行中に harness desktop smoke 8 件が stack overflow で abort した。対象の live / game scenario は上記の単独 entrypoint で成功済み。CI の必須 job を最終判定に使う。
- PR head の独立監査と CI は PR 作成後に追記する。
