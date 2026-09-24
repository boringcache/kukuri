/// 単一 scope（topic / channel）を ingest した結果のサマリ（監査 / テスト用）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestSummary {
    /// 走査した object state entry 数。
    pub scanned: usize,
    /// `allow` verdict で投影へ書いた entry 数。
    pub indexed: usize,
    /// fail-closed（unscanned / scan_failed / 非 allow / 取り込みの失敗）で投影しなかった entry 数。
    /// 一時的な失敗では既存 entry を保持する（#1090）。
    pub skipped_non_allow: usize,
    /// tombstone / deleted で de-index した entry 数。
    pub deindexed: usize,
    /// provider を呼んで判定した scan 数（post text + media blob。#1050）。
    pub scans_fresh: usize,
    /// 保存済みsubject判定または共通内容判定を再利用してproviderを呼ばなかったscan数。
    pub scans_reused: usize,
}

impl IngestSummary {
    pub(crate) fn merge(&mut self, other: Self) {
        self.scanned += other.scanned;
        self.indexed += other.indexed;
        self.skipped_non_allow += other.skipped_non_allow;
        self.deindexed += other.deindexed;
        self.scans_fresh += other.scans_fresh;
        self.scans_reused += other.scans_reused;
    }
}
