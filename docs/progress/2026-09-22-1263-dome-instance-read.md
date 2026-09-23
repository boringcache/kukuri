# #1263 Dome Instance の上限つき読み出し（2026-09-22）

- 対象 Issue: #1263
- Scope revision: `2026-09-22-approved-plan`
- 基準 commit: `70574678582d0ce79760819866f3cce63fa3165e`
- リスク区分: C（Instance state と署名 envelope の候補選択・検証経路を変更。authority の規則は変更しない）
- 仕様: ADR 0035、ADR 0036、ADR 0052、`AGENTS.md` の「設計原則: 件数に依存しない処理」

## 修正前の再現

`crates/app-api/src/tests/dome_listing.rs` に再現 test を先に追加し、修正前に次の3件が失敗することを確認した。

- `unreadable_unrelated_instance_does_not_break_game_room_listing`: 別 owner slot の読めない record により一覧全体が JSON decode error になった。
- `invalid_first_record_for_owner_slot_does_not_shadow_valid_instance`: 同じ key の先頭の不正 record により、後続の正しい Instance を読めなかった。
- `instance_lookup_reads_a_constant_number_of_docs_records`: 無関係な Instance を24件追加すると、1回の lookup が返す record 数が3件から27件へ増えた。

## 実装

- `hosting_instance` は `metaverse/dome-instances/` の prefix を走査しない。Spatial Context と owner から決定する owner slot の key を、最大8 record の上限つきで読む。
- owner が既知の一覧は `hosting_instance_for_owner` を直接使う。ID だけを受け取る照会は、自分の決定的な Instance ID、または `sessions/game/<id>/state` の上限つきの検証済み room から owner の手がかりを得る。手がかりだけでは採用せず、Instance ID・Spatial Context・owner と署名済み Instance の一致を確認する。
- Instance state と `envelopes/<id>` は、先頭1件ではなく最大8件の候補を検査する。読めない JSON、署名不正、owner・state・manifest の不一致は warn を出してその候補だけを除外し、検証済み候補のうち generation と更新時刻が最新のものを使う。
- docs/blob の I/O error は呼び出し側へ返す。blob または envelope が未着の場合は、従来の `DomeReadUnavailable` として対象だけを保留する。
- `list_game_rooms_scoped` は、同じ行について検証済み Instance を hosting view の生成まで渡す。従来の Instance 二重解決を除去した。
- `dome_mutations` の lock の取得位置・範囲・順序、Dome の署名・owner・generation・lifecycle の規則は変更していない。

## 固定 surface inventory

| ID | 入口 | 修正後の経路 | 対応 |
| --- | --- | --- | --- |
| INV-1 | `list_game_rooms_scoped` | row の owner → owner slot → Instance envelope。検証済み Instance を hosting view へ再利用 | AC-1〜4 / INVAR-1 |
| INV-2 | `get_dome_hosting`、`start_owner_dome_hosting_unlocked`、`prepare_community_node_dome_hosting_unlocked`、`activate_community_node_dome_hosting`、`close_dome_hosting_unlocked`、`submit_dome_session_input`、`commit_dome_layout` | 自分の決定的 ID、または検証済み game room から owner を得て owner slot を読む | AC-1・2・4 / INVAR-1・3 |
| INV-3 | `get_dome_hosting_authority`、`set_private_channel_entry_dome` | 同上 | AC-1・2・4 / INVAR-1 |
| INV-4 | `fetch_dome_instance_manifest` の caller（move・delete・connections・management を含む） | owner slot と envelope の各 key を最大8 record 読み、全候補を検証 | AC-1・4 / INVAR-1・2 |

`rg "hosting_instance\\(" crates/app-api/src` で定義を除く10 caller を確認した。`fetch_dome_instance_manifest` の直接 caller も逆引きし、すべて同じ上限つき候補検証を通る。未分類は0。

## 状態遷移の証跡

- TR-1: 正常 Instance と別 key の読めない Instance が混在しても、正常な Dome と通常の game room を一覧できる。
- TR-2: 無関係な Instance を24件増やしても、1 lookup の docs record 数は不変で、Instance prefix query は0回。
- TR-3: owner slot または envelope の先頭に不正 record があっても、上限内の正しい候補で一覧できる。
- TR-4: Instance・Preset・envelope が未着の既存 test は対象だけを保留し、到着後に回復する。
- I/O: Instance state の docs read と Instance blob の read が失敗した場合は `Err` のまま。

## P-13 の残り

`docs/architecture/replica-read-inventory.md` の P-13 は一部解消へ更新した。接続 topology の Instance 一覧、提案・選択・接続、hosting record、削除、layout commit の prefix 読みは本 Issue の Non-goal として残る。

## ローカル検証（Windows）

- 修正前: 追加した再現 test 3件が失敗（上記）。
- `cargo test -p kukuri-app-api dome_ -- --nocapture`: 34件成功。
- `cargo test -p kukuri-app-api dome_listing`: 15件成功。
- `cargo test -p kukuri-app-api forged_metaverse_room_is_not_listed_as_the_victims_dome`: 1件成功。
- `cargo xtask check`: 成功（Rust workspace Clippy、Tauri backend check、frontend lint/typecheck）。初回は `apps/desktop/node_modules` 未導入で frontend lint の開始時に停止し、runbook の `npx pnpm@10.16.1 install --dir apps/desktop` 後に再実行して成功した。
- `cargo xtask test`: Rust 1,330件成功・5件 skip、doc test 成功、frontend 1,984件成功。
- `cargo xtask app-api-slow-test`: 443件成功（Iroh integration tests を含む）。
- `cargo xtask oversized-files`: 成功。`dome_hosting.rs` は baseline 1,217行から1,193行へ縮小。
- `git diff --check`: 成功。
- envelope の不正候補にも warn を残す最終差分の後、Dome listing 15件、偽装 Dome contract 1件、`cargo xtask check`、`cargo xtask app-api-slow-test`（443件）、`cargo xtask oversized-files`、`git diff --check` を再実行して成功した。
