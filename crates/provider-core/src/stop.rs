/// Why a provider stopped discovering more search pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EmptyPage,
    NoNewCandidates,
    LastPage,
    MaxPages,
    MaxResults,
    Blocked,
    Captcha,
    RateLimited,
    Cancelled,
}

impl StopReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyPage => "empty_page",
            Self::NoNewCandidates => "no_new_candidates",
            Self::LastPage => "last_page",
            Self::MaxPages => "max_pages",
            Self::MaxResults => "max_results",
            Self::Blocked => "blocked",
            Self::Captcha => "captcha",
            Self::RateLimited => "rate_limited",
            Self::Cancelled => "cancelled",
        }
    }
}
