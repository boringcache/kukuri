## 概要

<!-- 具体的な問題と変更後の結果を書く。区分Aは概要・対象・確認結果を短く示せばよい。 -->

## 対象と条件

<!-- AGENTS.mdの作業原則・設計原則を適用し、手順はdocs/runbooks/issue-lifecycle.mdに従う。1PRは事前定義した1つの末端ACに対応する。非該当欄は削除してよい。
必要な検証の未実行・失敗・例外は削除しない。詳細がrepositoryにあればリンクする。
区分Cは自動Close文言を使わず Refs を使い、PR headの独立監査PASSと必須CIの成功後にマージする。 -->

- 対象Issue（Refs / Closes）・種別（fix / feature / refactor / contract / scenario / docs / deps）:
- リスク区分・Scope revision・基準commit:
- 変更path・対象外:
- 対応する1つの末端AC・予定PR枠 / 維持するINVARと差分・検証証跡の対応:
- 統合・置換・削除した処理、残る暫定処理の撤去条件、完成後に残るコードの必要性:
- B/C: inventoryの探索方法、変更前後、未分類件数・状態遷移:
- C: sensitive sinkとshared helperの全caller確認:

## 変更前後の証拠

<!-- fixはrunbookの「修正前の再現」に従う。自動化可能ならfailing-before / passing-after。
実機・視覚のみなら再現条件・変更前の観測・期待結果・変更後の結果・自動化不能の理由。
文書の不一致は該当箇所と同じ依頼への適用結果。refactorはREFACTORING.mdの構造上の成果と挙動維持の証跡へリンクする。 -->

## UI変更

<!-- UI変更がなければ節ごと削除。正本は docs/adr/0014-uiux-dev-flow.md。
小さな文言修正は対象locale・期待結果・表示確認を短く記載し、非該当欄を省略できる。 -->

- 変更分類・対象surface・利用者・単一目的・主要操作:
- 維持する挙動（scope / focus / draft / scroll / 戻る文脈）・非目標:
- 対象platform / viewport / theme / locale / state:
- 同条件のbefore / after画像または動画:
- Accessibilityと実操作（keyboard / pointer / touch / screen reader）:
- 性能影響と計測:
- UI review record（ADR 0014で必要な場合）:
- 未確認事項・理由・DESIGN / ADR 0014からの例外:

## 検証とリスク

<!-- ローカルは変更関連箇所、全体確認はPR CI。REFACTORING.mdから必要な検証を選ぶ。
CI成功だけをAC達成の証拠にしない。未依頼のエッジケースを追加しない。 -->

- 実行command・結果:
- 未実行・中断・失敗と理由、補完方法:
- CI（対象commit・結果）:
- 挙動・互換性への影響、残存リスク:
- 追加発見の分類（Existing-gap / Regression / New-requirement / Optional-hardening）:

## 独立監査

<!-- 必要な場合だけ残す。Cは固定条件の独立監査を行い、Bは計画時に必要性を定める。
対象commit、Scope revision、inventoryの合計/適合/不適合/未分類、AC/INVAR evidence、validation、blocker、non-blocker、PASS/FAIL/INCONCLUSIVEを記録した監査へリンクする。段階移行では旧経路・暫定選択器・台帳・無上限queueの担当条件と撤去状況、最終コード量の比較を固定した完成形と照合する。監査後の変更はdeltaを再監査する。 -->

- 監査記録・対象commit・判定:
- 完成形への到達、撤去・回収・上限と総コード量の比較（該当時）:
- 監査後delta:
