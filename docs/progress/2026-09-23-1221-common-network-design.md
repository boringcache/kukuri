# #1221 共通通信設計の判断反映

## 現在の成果

- 基準: `984a491a3430f1da105bfe8ff49a877cbab4690f`。
- P1は完了済み。P2は進行中であり、製品コード・writer・受信経路は切り替えていない。
- ユーザー決定: D2は現在の受信対象を維持。D10は更新案内後bucket切替時に旧版との新着相互運用を終了。
- [ADR 0055](../adr/0055-demand-owned-network-work.md)へowner/需要/容量/停止/受信/移行を記録。
- [inventory](../architecture/network-work-inventory.md)へ既知の28groupと上流9項目、残確認を記録。
  全件列挙完了・未分類0とは扱わない。
- ADR 0054の互換方針と旧子Issueの管理参照を同期し、docs入口・runbookから新設計へ接続した。

## 限定した独立設計レビュー

区分C、Scope revision `2026-09-22-consolidated-v2`。
担当 `/root/design_audit` は実装に関与せず、ADR 0055 §1〜5の未commit草案を
現行ADR 0020/0023/0054と照合した。これは固定PR headの実装監査や全P2完了監査ではない。

| 指摘 | 初回判定 | 対応とdelta判定 |
| --- | --- | --- |
| DR-1 accountからendpointへの発見契約がない | D2/NET-INVAR/DM offlineのExisting-gap | 署名binding、QUIC identity照合、CN候補の検証、CNなしmanual/seed/DHT経路とendpoint更新を§4へ記録。設計上解消 |
| DR-2 旧DM宛先のままでは新受信routeへ届かない | D2/ADR0020のExisting-gap | message ID/暗号frame不変で輸送先だけ移行、ACK経路・重複/tombstoneを§4/5へ記録。設計上解消 |
| DR-3 非表示private channelの新epoch鍵に移れない | D2/private INVARのExisting-gap | 既知旧epochで保護した制御capsule、個別grant検証、連続rotationのcursor再開を§4へ記録。設計上解消 |

初回限定レビューはFAIL 3件、その3件へのdeltaレビューは全件解消。
未知の不具合を探す全レビューの反復は行わない。実Iroh・2端末の実証、wire値、全P2の監査は未実施。

## 実装前に残る技術確認

1. 上流の指定peer限定/neighbor受付/進行中sync停止/内部接続容量を実現するAPI。
   現行iroh-docsのstart_syncは保存peerを追加するため、既存leave/startの組合せでは不足する。
2. 受信routeのbinding・暗号capsule・private制御配送のwire contractと2端末の実証。
3. inventoryの未分類入口・sink caller・writer/GC/UIを完成し、固定ACへ対応させる。

これらは承認済み範囲の技術作業であり、ユーザーの再承認待ちではない。
局所変更は関連検証をローカル実行し、全体確認はPR/CIで行う。

今回の文書差分は `git diff --cached --check` と変更6文書のローカル参照31件を確認した。
製品コード変更がないため製品テストは実行していない。Issue本文の蓄積したCR/空行も、
非空白文字が全て同じこととGitHubへ保存された本文の一致を確認して正規化した。

## P2実証: accountとendpointの対応確認

ユーザーの自律続行指示に従い、DR-1のうち接続先を認証する契約を実装した。
新しいALPNのhandler/clientと署名型を追加するが、productionのRouterへはまだ登録しない。
既存のtopic/DM/private受信、writer、保存形式は切り替えていない。

| 固定条件 | 実装 / 証拠 |
| --- | --- |
| BIND-1: account/route/endpoint/時刻を署名で束縛し、別accountやendpointの自己申告を採用しない（NW-8/DR-1） | `ReceiveEndpointBindingV1::verify_for`、`receive_endpoint_binding_covers_every_field_with_signature`、実QUICの`receive_binding_replay_from_another_endpoint_is_rejected` |
| BIND-2: 期限/版/入力bytesを制限し、未知field・無効な時刻・過大入力を拒否する（NW-2/3） | coreの`expires_and_rejects_invalid_intervals`/`decode_is_bounded_and_strict`。wire 1,024bytes、寿命5分、未来許容1分 |
| BIND-3: CNを使わず、実endpointへ接続して対応を確認する。同一accountの複数端末を排除しない（NET-INVAR-1/NW-8） | `receive_binding_exchange_uses_authenticated_endpoint_without_cn`、coreの`allows_multiple_devices_without_changing_route` |
| BIND-4: protocol受付超過は待機せず拒否し、caller取消時に短期QUIC接続を閉じる（NW-2/4） | `receive_binding_full_server_rejects_instead_of_waiting`、`receive_binding_cancel_closes_the_connection` |
| BIND-5: binding更新でaccount/endpointを切り替えず、古い発行時刻へ戻さない（NW-7） | `ReceiveBindingProtocol::replace`、`receive_binding_replacement_cannot_switch_account_or_endpoint` |

対象入口・sinkはinventoryのN29〜31。新しい要求内容ではなく、既存NW-2/3/4/7/8のこの実装単位への対応である。
handlerの同時2件はQUIC handshake全体の上限を証明しない。共通ownerへの組込みと、通知参照の暗号化・配送・
private制御配送・旧DM outbox移行は未完了としてP2/P3へ残す。

ローカル検証:

- `cargo test -p kukuri-core --lib receive_endpoint_binding`: 5件成功。
- `cargo test -p kukuri-transport --lib receive_binding`: 5件成功。CNなしの実Iroh接続を含む。
- `cargo clippy -p kukuri-core -p kukuri-transport --all-targets -- -D warnings`: 成功。
- 全体testは実行しない。固定headの独立監査とPR/CIで全体確認を行う。
