use criterion::{Criterion, criterion_group, criterion_main};
use intervals_icu_client::{IntervalsClient, http_client::ReqwestIntervalsClient};
use secrecy::SecretString;
use tempfile::tempdir;
use tokio::runtime::Builder;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn bench_download_activity_file(c: &mut Criterion) {
    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let server = rt.block_on(async {
        let server = MockServer::start().await;
        let body = vec![7u8; 256 * 1024]; // 256KB payload to exercise streaming path
        Mock::given(method("GET"))
            .and(path("/api/v1/activity/a1/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        server
    });

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    c.bench_function("download_activity_file_stream", |b| {
        b.to_async(&rt).iter(|| {
            let client = client.clone();
            let tmpdir = tempdir().expect("tempdir");
            let path = tmpdir.path().join("out.bin");
            async move {
                let _hold_dir = tmpdir; // keep tempdir alive until future completes
                client
                    .download_activity_file("a1", Some(path.clone()))
                    .await
                    .expect("download");
                let _ = tokio::fs::metadata(&path).await.expect("metadata");
            }
        })
    });
}

criterion_group!(benches, bench_download_activity_file);
criterion_main!(benches);
