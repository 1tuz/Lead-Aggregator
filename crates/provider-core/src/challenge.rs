use regex::Regex;
use twogis_domain::{ErrorKind, SourceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeKind {
    None,
    Captcha,
    AntiBot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeDetection {
    pub detected: bool,
    pub kind: ChallengeKind,
    pub confidence: ChallengeConfidence,
    pub reason: String,
    pub page_title: Option<String>,
}

impl ChallengeDetection {
    pub fn clear(page_title: Option<String>) -> Self {
        Self {
            detected: false,
            kind: ChallengeKind::None,
            confidence: ChallengeConfidence::Low,
            reason: String::new(),
            page_title,
        }
    }

    pub fn error_kind(&self) -> Option<ErrorKind> {
        if !self.detected {
            return None;
        }
        Some(match self.kind {
            ChallengeKind::Captcha => ErrorKind::CaptchaRequired,
            ChallengeKind::AntiBot => ErrorKind::ChallengeRequired,
            ChallengeKind::None => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChallengeEvidence<'a> {
    pub source: SourceKind,
    pub http_status: Option<u16>,
    pub request_url: &'a str,
    pub final_url: Option<&'a str>,
    pub html: &'a str,
}

pub fn detect_challenge(evidence: &ChallengeEvidence<'_>) -> ChallengeDetection {
    let title = extract_title(evidence.html);
    let effective_url = evidence.final_url.unwrap_or(evidence.request_url);
    let visible = visible_text(evidence.html);

    if let Some(reason) = challenge_url_reason(effective_url) {
        return ChallengeDetection {
            detected: true,
            kind: ChallengeKind::Captcha,
            confidence: ChallengeConfidence::High,
            reason: format!("challenge URL: {reason}"),
            page_title: title,
        };
    }

    if let Some(reason) = challenge_title_reason(title.as_deref()) {
        let kind = if reason.contains("captcha") {
            ChallengeKind::Captcha
        } else {
            ChallengeKind::AntiBot
        };
        return ChallengeDetection {
            detected: true,
            kind,
            confidence: ChallengeConfidence::High,
            reason: format!("page title indicates challenge ({reason})"),
            page_title: title,
        };
    }

    if let Some(reason) = captcha_widget_reason(evidence.html) {
        return ChallengeDetection {
            detected: true,
            kind: ChallengeKind::Captcha,
            confidence: ChallengeConfidence::High,
            reason: format!("CAPTCHA widget present ({reason})"),
            page_title: title,
        };
    }

    if let Some(reason) = visible_challenge_reason(&visible) {
        return ChallengeDetection {
            detected: true,
            kind: ChallengeKind::Captcha,
            confidence: ChallengeConfidence::High,
            reason: format!("visible challenge text ({reason})"),
            page_title: title,
        };
    }

    // Soft signal: expected catalog structure missing together with interstitial markers.
    if !has_expected_catalog_markers(evidence.source, evidence.html)
        && interstitial_markers(evidence.html, &visible)
    {
        return ChallengeDetection {
            detected: true,
            kind: ChallengeKind::AntiBot,
            confidence: ChallengeConfidence::Medium,
            reason: "missing catalog markup with anti-bot interstitial markers".into(),
            page_title: title,
        };
    }

    ChallengeDetection::clear(title)
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title")?;
    let after = &html[start..];
    let open_end = after.find('>')?;
    let rest = &after[open_end + 1..];
    let close_rel = rest.to_lowercase().find("</title>")?;
    let raw = rest[..close_rel].trim();
    if raw.is_empty() {
        None
    } else {
        Some(collapse_ws(raw))
    }
}

fn visible_text(html: &str) -> String {
    // Strip script/style blocks so embedded "/captcha/widget.js" never counts as visible CAPTCHA.
    let without_script = strip_tag_blocks(html, "script");
    let without_style = strip_tag_blocks(&without_script, "style");
    let without_tags = Regex::new(r"(?is)<[^>]+>")
        .ok()
        .map(|re| re.replace_all(&without_style, " ").into_owned())
        .unwrap_or(without_style);
    collapse_ws(&decode_basic_entities(&without_tags)).to_lowercase()
}

fn strip_tag_blocks(html: &str, tag: &str) -> String {
    let Ok(re) = Regex::new(&format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}>")) else {
        return html.to_owned();
    };
    re.replace_all(html, " ").into_owned()
}

fn decode_basic_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn challenge_url_reason(url: &str) -> Option<&'static str> {
    let lower = url.to_lowercase();
    const MARKERS: &[&str] = &[
        "/showcaptcha",
        "/checkcaptcha",
        "/captcha?",
        "/captcha/",
        "/challenge",
        "/verify",
        "captcha.yandex",
        "geo.captcha",
        "recaptcha",
        "hcaptcha.com",
    ];
    MARKERS
        .iter()
        .find(|marker| lower.contains(*marker))
        .copied()
}

fn challenge_title_reason(title: Option<&str>) -> Option<String> {
    let title = title?.to_lowercase();
    const MARKERS: &[&str] = &[
        "captcha",
        "are you a robot",
        "just a moment",
        "attention required",
        "access denied",
        "verify you are human",
        "проверка безопасности",
        "не робот",
        "подтвердите, что вы",
        "доступ ограничен",
        "временно недоступ",
    ];
    MARKERS
        .iter()
        .find(|marker| title.contains(*marker))
        .map(|marker| (*marker).to_owned())
}

fn captcha_widget_reason(html: &str) -> Option<&'static str> {
    let lower = html.to_lowercase();
    // Require interactive widget markup — a script src with "captcha" alone is not enough.
    if lower.contains("iframe")
        && (lower.contains("recaptcha")
            || lower.contains("hcaptcha")
            || lower.contains("smartcaptcha")
            || lower.contains("captcha-iframe")
            || lower.contains("showcaptcha"))
    {
        return Some("captcha iframe");
    }
    if lower.contains("g-recaptcha") || lower.contains("class=\"g-recaptcha\"") {
        return Some("g-recaptcha");
    }
    if lower.contains("h-captcha") {
        return Some("h-captcha");
    }
    if lower.contains("smartcaptcha") && lower.contains("data-sitekey") {
        return Some("yandex smartcaptcha");
    }
    if lower.contains("cf-turnstile") {
        return Some("cloudflare turnstile");
    }
    None
}

fn visible_challenge_reason(visible: &str) -> Option<&'static str> {
    const PHRASES: &[&str] = &[
        "подтвердите, что вы не робот",
        "подтвердите что вы не робот",
        "я не робот",
        "проверка безопасности",
        "пройдите проверку",
        "are you a robot",
        "verify you are human",
        "complete the captcha",
        "please complete the security check",
        "unusual traffic from your computer",
    ];
    PHRASES
        .iter()
        .find(|phrase| visible.contains(*phrase))
        .copied()
}

fn interstitial_markers(html: &str, visible: &str) -> bool {
    let lower = html.to_lowercase();
    visible.contains("доступ ограничен")
        || visible.contains("подозрительная активность")
        || visible.contains("request blocked")
        || visible.contains("security check")
        || lower.contains("id=\"challenge-form\"")
        || lower.contains("name=\"captcha\"")
}

fn has_expected_catalog_markers(source: SourceKind, html: &str) -> bool {
    let lower = html.to_lowercase();
    match source {
        SourceKind::TwoGis => {
            lower.contains("/firm/")
                || lower.contains("2gis.ru")
                || lower.contains("data-id=\"firm\"")
        }
        SourceKind::Yell => {
            lower.contains("yell.ru") || lower.contains("/com/") || lower.contains("company")
        }
        SourceKind::Zoon => {
            lower.contains("zoon.ru") || lower.contains("/m/") || lower.contains("item")
        }
        SourceKind::Rusprofile => {
            lower.contains("rusprofile")
                || lower.contains("огрн")
                || lower.contains("инн")
                || lower.contains("/id/")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence<'a>(
        source: SourceKind,
        html: &'a str,
        final_url: &'a str,
    ) -> ChallengeEvidence<'a> {
        ChallengeEvidence {
            source,
            http_status: Some(200),
            request_url: final_url,
            final_url: Some(final_url),
            html,
        }
    }

    #[test]
    fn script_captcha_widget_src_is_not_captcha() {
        for source in [
            SourceKind::TwoGis,
            SourceKind::Yell,
            SourceKind::Zoon,
            SourceKind::Rusprofile,
        ] {
            let html = r#"
              <html><head><title>Поиск компаний</title>
              <script src="/captcha/widget.js"></script>
              <style>.captcha-hint { color: red }</style>
              </head><body>
                <a href="/moscow/firm/7000000001">Автосервис</a>
                <a href="https://www.yell.ru/moscow/com/demo/">Demo</a>
                <a href="https://zoon.ru/msk/m/demo/">Zoon</a>
                <div>ИНН 7707083893 ОГРН 1027700132195</div>
              </body></html>
            "#;
            let detection =
                detect_challenge(&evidence(source, html, "https://example.test/search"));
            assert!(
                !detection.detected,
                "{source:?} false positive: {}",
                detection.reason
            );
        }
    }

    #[test]
    fn real_captcha_title_is_detected() {
        for source in [
            SourceKind::TwoGis,
            SourceKind::Yell,
            SourceKind::Zoon,
            SourceKind::Rusprofile,
        ] {
            let html = r#"<html><head><title>CAPTCHA</title></head><body><p>Подтвердите, что вы не робот</p></body></html>"#;
            let detection =
                detect_challenge(&evidence(source, html, "https://example.test/search"));
            assert!(detection.detected);
            assert_eq!(detection.kind, ChallengeKind::Captcha);
        }
    }

    #[test]
    fn recaptcha_widget_is_detected() {
        let html = r#"
          <html><head><title>Security</title></head>
          <body><div class="g-recaptcha" data-sitekey="x"></div></body></html>
        "#;
        let detection = detect_challenge(&evidence(
            SourceKind::TwoGis,
            html,
            "https://2gis.ru/search",
        ));
        assert!(detection.detected);
        assert_eq!(detection.kind, ChallengeKind::Captcha);
    }

    #[test]
    fn challenge_url_is_detected() {
        let html = "<html><head><title>OK</title></head><body>loading</body></html>";
        let evidence = ChallengeEvidence {
            source: SourceKind::Yell,
            http_status: Some(200),
            request_url: "https://www.yell.ru/search",
            final_url: Some("https://www.yell.ru/showcaptcha?d=1"),
            html,
        };
        let detection = detect_challenge(&evidence);
        assert!(detection.detected);
        assert_eq!(detection.kind, ChallengeKind::Captcha);
    }

    #[test]
    fn normal_catalog_page_passes() {
        let html = r#"
          <html><head><title>2ГИС — поиск</title></head>
          <body><a href="/moscow/firm/7000000001">СТО</a></body></html>
        "#;
        let detection = detect_challenge(&evidence(
            SourceKind::TwoGis,
            html,
            "https://2gis.ru/moscow/search",
        ));
        assert!(!detection.detected);
    }

    #[test]
    fn http_style_cases_are_classified_by_caller_separately() {
        // Detector itself is HTML/URL based; HTTP 403/429 are handled in CatalogHttpClient.
        let html = r#"<html><head><title>Catalog</title></head><body><a href="/firm/1">A</a></body></html>"#;
        assert!(
            !detect_challenge(&evidence(SourceKind::TwoGis, html, "https://2gis.ru/x")).detected
        );
    }
}
