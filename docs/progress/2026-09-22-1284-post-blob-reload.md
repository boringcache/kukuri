# Issue #1284: 投稿blobの局所再読み込み

## Scope

- Scope revision: `2026-09-22-1284-v1`
- 基準commit: `b39d3b606d835aaae9580b13b85d3742e78cb2b6`
- リスク区分: C（利用者操作からP2P blob取得を起動するnetwork経路）
- Issue: [#1284](https://github.com/kukuri-app/kukuri/issues/1284)

## 実装

- Timeline / Thread / Profileの上限付き表示範囲と、20件単位でページ置換するBookmarksを含むviewport内PostCardが参照する直前の返信先について、projection行はあるが本文が欠けている場合も`MissingBodyLedger`による有限取得へ入れる。
- `retry_post_elements`は対象投稿自身またはその直前の返信先だけを受け付け、欠けた本文を1回明示再試行して更新後の`PostView`を返す。replica／projectionの走査は行わない。
- `ReplyPreviewView.content_status`で返信先本文の欠損を文字列判定から分離した。
- 投稿本文・返信先本文の失敗箇所に局所再読み込みを表示し、投稿操作群の右端にカード単位の再読み込みを追加した。カード単位では失敗済み添付hashも既存のmedia明示再試行へ渡す。
- 更新結果は操作した`PostCard`へ局所反映する。全timeline cacheの走査、scroll／route／draftの変更は行わない。

## AC / INVAR evidence

| 条件 | 実装・test |
| --- | --- |
| AC-1 | `reflect_reply_targets_for_rows` / `reflect_reply_targets_for_profile_items`、viewport内PostCardの有限backoff recovery、`a_visible_reply_target_with_a_missing_body_recovers_after_its_provider_returns`、`profile_view_recovers_the_visible_reply_target_body`、`visible missing reply preview follows the bounded automatic recovery schedule` |
| AC-2 / AC-5 | `retry_post_elements`、`MissingBodyLedger::request_manual_retry`、`manual_post_body_retry_bypasses_the_automatic_cooldown_once`、`PostCard.test.tsx` |
| AC-3 / AC-4 | `PostReloadContext`、`PostCard`右端action、`post reload stays busy until its failed media retry settles` |
| INVAR-1 | withdrawal、scope不一致、private membership、adult/advisory gateを取得前に除外。container自身または直前の返信先以外を拒否し、negative testで禁止I/O 0を確認 |
| INVAR-2 | 表示ページの行数と1カード内の要素だけを処理。Bookmarksは20件のページ置換で、observer・timerを総件数に比例させない。全件scanなし |
| INVAR-3 | 更新後viewは操作中の`PostCard`だけへ反映。`late reload result does not cross a backend or scope change`で遅着結果を破棄 |
| INVAR-4 | 自動台帳を共有し、明示操作は1試行。mediaは既存`retryMediaFetch`を利用 |

## Validation

| command / evidence | 結果 |
| --- | --- |
| 修正前 `a_visible_reply_target_with_a_missing_body_recovers_after_its_provider_returns` | `[blob pending]` のままで失敗することを確認。修正後PASS |
| `cargo xtask check` | PASS（fmt、clippy、Tauri compile、frontend lint / typecheck） |
| `cargo xtask rust-test` | PASS（nextest 1,324件、skip 5件、全doc-test） |
| `cargo xtask app-api-slow-test` | PASS（実Irohを含む439件） |
| `cargo xtask tauri-test` | PASS（77件、Windows、rebase後） |
| `cargo xtask e2e-smoke` | PASS（`desktop_smoke_post_persist`） |
| `cargo xtask desktop-ui-check` | Vitest 248 files / 1,994件、Storybook buildはPASS。browser Playwrightは368/371件PASS後、無関係のtimeout/crash 3件を対象群27件で再実行してPASS |
| 最終 targeted Vitest | PASS（3 files / 65件。自動再取得が8試行で停止するassertionを含む） |
| 最終 `pnpm test:e2e:visual` | PASS（44件。Windowsではsnapshot比較を行わない既存設定のため到達・描画smoke） |
| `cargo test -p kukuri-cli --test command_parity` | PASS（5件、GUI専用回復commandを`gui_content_recovery`として分類） |
| `cargo xtask ipc-types` | PASS、Rust由来のTypeScript型を再生成 |
| `cargo xtask oversized-files` | PASS。reload hook / reply context / reload test / runtime APIを分割し、既存baselineを増やしていない |

Windowsではvisual snapshot比較を行わない既存設定のため、visual 44件は到達・描画smokeとして成功した。Linux CIで右端reload iconによる`author-pane-wide-dark.png`の意図した差分を確認し、CI artifactのactualを正本snapshotへ更新した。native WebView固有機能は変更しておらず、Tauri command登録・compile・lib testとbrowser実操作を確認した。

PR head、独立監査、CI、merge commitはPR工程で追記する。

## 独立監査で修正したblocker

rebase前headの監査で、次を固定AC / INVARのblockerとして検出し、新headへ修正した。

- adult/advisory gate中にも右端reloadが表示され、本文IPCへ到達できたため、gate中は操作を出さず、backendでもself-label本文をI/O前に拒否した。
- 自動返信先回復がwithdrawalとscope一致をblob確認後まで検証していなかったため、withdrawal・topic/channel・private membershipをI/O前guardへ移した。
- Profileが共通helperのcallerに含まれていなかったため、上限付きprofile pageから作った参照を同じ有限retryへ渡した。Bookmarks APIは未ページングの全件取得なのでbackendで全行を処理せず、表示を20件単位の置換型ページに制限した上で、viewportへ入った欠損previewのPostCardだけがautomatic modeで同じ台帳へ有限backoff要求する。
- カードのbusyが本文IPC完了だけで解除されていたため、受理されたmedia retryのin-flight終了も待つようにした。
- backend / scope切替後の遅着responseを捨てるため、開始時のreload contextとPostView参照を完了時に再検証した。
