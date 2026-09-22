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
