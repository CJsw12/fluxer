use super::*;

#[test]
fn locale_resolution_and_available() {
    assert_eq!(available_locales().len(), 44);
    assert_eq!(resolve_locale("ko-KR"), "ko");
    assert_eq!(resolve_locale("ko_KR.UTF-8"), "ko");
    assert_eq!(resolve_locale("zh-Hant-TW"), "zh-TW");
    assert_eq!(resolve_locale("pt-PT"), "pt-PT");
    assert_eq!(resolve_locale("pt-BR"), "pt-BR");
    assert_eq!(resolve_locale("nb-NO"), "no");
    assert_eq!(resolve_locale("xx-YY"), "en-US");
    assert_eq!(resolve_locale(""), "en-US");
}

#[test]
fn korean_lookups() {
    let ko = I18n::new("ko");
    assert_eq!(ko.t("Log out"), "로그아웃");
    assert_eq!(
        ko.t_args("You're here, {username}! Good to have you with us.", &[("username", "민수")]),
        "민수님, 오셨군요! 함께하게 되어 반갑습니다."
    );
    assert_eq!(ko.t_args("Message #{channelName}", &[("channelName", "일반")]), "#일반에 메시지 보내기");
}

#[test]
fn english_lookups_use_source() {
    let en = I18n::new("en-US");
    assert_eq!(en.t("Log out"), "Log out");
    assert_eq!(
        en.t_args("You're here, {username}! Good to have you with us.", &[("username", "Sam")]),
        "You're here, Sam! Good to have you with us."
    );
    assert_eq!(en.t_args("Message #{channelName}", &[("channelName", "general")]), "Message #general");
    assert_eq!(t!(en, "Message #{channelName}", channelName = "dev"), "Message #dev");
}

#[test]
fn plurals() {
    let id = "{count, plural, one {# member} other {# members}}";
    let en = I18n::new("en-US");
    assert_eq!(en.t_args(id, &[("count", "1")]), "1 member");
    assert_eq!(en.t_args(id, &[("count", "5")]), "5 members");
    let ru = I18n::new("ru");
    assert_eq!(ru.t_args(id, &[("count", "1")]), "1 участник");
    assert_eq!(ru.t_args(id, &[("count", "3")]), "3 участника");
    assert_eq!(ru.t_args(id, &[("count", "5")]), "5 участников");
    assert_eq!(ru.t_args(id, &[("count", "21")]), "21 участник");
    let ko = I18n::new("ko");
    assert_eq!(ko.t_args(id, &[("count", "1")]), "1명");
    assert_eq!(ko.t_args(id, &[("count", "7")]), "7명");
}

#[test]
fn exact_offset_select_and_quotes() {
    let en = I18n::new("en-US");
    let m = "{n, plural, =0 {none} one {# item} other {# items}}";
    assert_eq!(en.t_args(m, &[("n", "0")]), "none");
    assert_eq!(en.t_args(m, &[("n", "1")]), "1 item");
    assert_eq!(en.t_args("{g, select, f {she} other {they}}", &[("g", "f")]), "she");
    assert_eq!(en.t_args("{g, select, f {she} other {they}}", &[("g", "x")]), "they");
    assert_eq!(en.t("Use '{'braces'}' it''s"), "Use {braces} it's");
    assert_eq!(en.t("Hello {name}"), "Hello {name}");
}

#[test]
fn missing_key_falls_back_to_source() {
    let ko = I18n::new("ko");
    assert_eq!(ko.t("This string does not exist in any catalog"), "This string does not exist in any catalog");
    assert_eq!(ko.t_args("Not in catalog {x}", &[("x", "1")]), "Not in catalog 1");
}

#[test]
fn context_lookup() {
    let ko = I18n::new("ko");
    assert_eq!(ko.t_ctx("short-relative-time", "{count, plural, one {#d} other {#d}}", &[("count", "3")]), "3일");
}

#[test]
fn system_locale_resolves() {
    let sys = I18n::from_system();
    assert!(available_locales().contains(&sys.locale()));
}

#[test]
fn embedded_size_report() {
    let total: usize = KEYS_ZST.len() + LOCALES.iter().map(|t| t.zst.len()).sum::<usize>();
    eprintln!("embedded compressed bytes: {total}");
    assert!(total < 8 * 1024 * 1024);
}

#[test]
fn only_requested_locale_is_decompressed() {
    // Other tests use en-US, ko and ru; ja and de are touched only here.
    let loaded = |c: &str| table(c).is_some_and(|t| t.is_loaded());
    assert!(!loaded("ja") && !loaded("de"));
    let t0 = std::time::Instant::now();
    let ja = I18n::new("ja");
    let _ = ja.t("Log out");
    eprintln!("first ja lookup: {:?}", t0.elapsed());
    assert!(loaded("ja"));
    assert!(!loaded("de"));
    let others: Vec<_> = LOCALES.iter().filter(|t| t.is_loaded()).map(|t| t.code).collect();
    assert!(others.iter().all(|c| ["ja", "en-US", "ko", "ru"].contains(c)), "{others:?}");
}
