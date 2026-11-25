use intervals_icu_client::{
    IntervalsClient,
    http_client::{IntervalsTransport, ReqwestIntervalsClient},
};
use secrecy::SecretString;

#[tokio::test]
async fn client_error_resets_consecutive_server_failures() {
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let transport = IntervalsTransport::new().unwrap();
    let client = ReqwestIntervalsClient::with_transport(
        &server.uri(),
        "i1",
        SecretString::new("key".into()),
        &transport,
    );
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    for _ in 0..4 {
        assert!(
            !client
                .get_athlete_profile()
                .await
                .unwrap_err()
                .to_string()
                .contains("circuit breaker open")
        );
    }
    server.reset().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert!(
        !client
            .get_athlete_profile()
            .await
            .unwrap_err()
            .to_string()
            .contains("circuit breaker open")
    );
    server.reset().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    for _ in 0..5 {
        assert!(
            !client
                .get_athlete_profile()
                .await
                .unwrap_err()
                .to_string()
                .contains("circuit breaker open")
        );
    }
    assert!(
        client
            .get_athlete_profile()
            .await
            .unwrap_err()
            .to_string()
            .contains("circuit breaker open")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn transport_breaker_is_shared_across_fresh_credentials_but_not_other_transports() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            drop(socket);
        }
    });
    let transport = IntervalsTransport::new().unwrap();
    for _ in 0..5 {
        let client = ReqwestIntervalsClient::with_transport(
            &base,
            "i1",
            SecretString::new("first".into()),
            &transport,
        );
        assert!(client.get_athlete_profile().await.is_err());
    }
    assert!(
        transport.is_in_use(),
        "an open circuit must not be evicted after its clients are dropped"
    );
    let rotated = ReqwestIntervalsClient::with_transport(
        &base,
        "i1",
        SecretString::new("second".into()),
        &transport,
    );
    let error = rotated.get_athlete_profile().await.unwrap_err();
    assert!(
        error.to_string().contains("circuit breaker open"),
        "{error}"
    );
    let isolated = IntervalsTransport::new().unwrap();
    let other = ReqwestIntervalsClient::with_transport(
        &base,
        "i2",
        SecretString::new("other".into()),
        &isolated,
    );
    let error = other.get_athlete_profile().await.unwrap_err();
    assert!(
        !error.to_string().contains("circuit breaker open"),
        "{error}"
    );
    server.abort();
}
