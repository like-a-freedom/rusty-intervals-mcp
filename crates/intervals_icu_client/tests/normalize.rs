use intervals_icu_client::http_client::ReqwestIntervalsClient;

#[test]
fn normalize_sport_lowercase() {
    assert_eq!(ReqwestIntervalsClient::normalize_sport("run"), "Run");
    assert_eq!(ReqwestIntervalsClient::normalize_sport("Ride"), "Ride");
    assert_eq!(
        ReqwestIntervalsClient::normalize_sport("virtualrun"),
        "VirtualRun"
    );
    assert_eq!(ReqwestIntervalsClient::normalize_sport(""), "");
}
