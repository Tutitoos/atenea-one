// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use atenea_identity::{AgentState, LocalKeys, generate_seed, signing_key, verify_config};
use common::{Device, TestApp, id, now};
use futures_util::{StreamExt, stream};
use serde_json::json;
use sqlx::{Postgres, QueryBuilder};
use std::time::Instant;
use uuid::Uuid;

// Prueba de carga optativa: el resto de la suite cubre las transiciones de aprobación por API.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requiere PostgreSQL; mide 500 renovaciones firmadas en una sola red"]
async fn quinientos_dispositivos_renuevan_sin_politicas_por_pareja() {
    let t = TestApp::new().await.expect("ATENEA_TEST_DATABASE_URL");
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let anchor = root.verifying_key().to_bytes();
    let network = t.json("POST", &format!("/api/v1/orgs/{org}/networks"), Some(&s), json!({"name":"escala sintética", "ipv4_cidr":"10.97.0.0/22", "ipv6":false, "root_anchor":hex::encode(anchor)})).await;
    assert!(network.status.is_success(), "{}", network.json);
    let net = id(&network.json);
    let detail = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}"),
            Some(&s),
            json!(null),
        )
        .await;
    assert_eq!(
        detail.json["auto_connect"], true,
        "una red nueva conecta por aprobación"
    );
    t.certificate(&s, net, &root).await;
    let devices: Vec<_> = (0..500).map(|_| Device::new()).collect();
    let mut seed = QueryBuilder::<Postgres>::new(
        "INSERT INTO devices (id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at,approval_order,auto_connect_on_approval,config_v3) ",
    );
    seed.push_values(devices.iter().enumerate(), |mut b, (i, d)| {
        b.push_bind(d.id)
            .push_bind(net)
            .push_bind("sintético")
            .push_bind("approved")
            .push_bind(d.bundle.noise_pubkey.as_slice())
            .push_bind(d.bundle.identity_pubkey.as_slice())
            .push_bind(d.bundle.encode().to_vec())
            .push("now()")
            .push_bind(i as i64 + 1)
            .push_bind(true)
            .push_bind(true);
    });
    seed.build().execute(&t.pool).await.unwrap();
    sqlx::query("INSERT INTO ip_allocations(network_id,device_id,family,addr) SELECT network_id,id,4,inet '10.97.0.0' + approval_order FROM devices WHERE network_id=$1")
        .bind(net).execute(&t.pool).await.unwrap();
    let t_ref = &t;
    let start = Instant::now();
    let mut samples = stream::iter(&devices)
        .map(|d| async move {
            let start = Instant::now();
            let res = t_ref.config(d, now(), *Uuid::new_v4().as_bytes()).await;
            let issue = start.elapsed();
            assert!(res.status.is_success(), "{}", res.json);
            let state = AgentState::new(*net.as_bytes(), *d.id.as_bytes(), anchor);
            let keys = LocalKeys {
                noise_pub: d.bundle.noise_pubkey,
                identity_pub: d.bundle.identity_pubkey,
            };
            let start = Instant::now();
            let checked = verify_config(&res.body, &keys, &state, now()).unwrap();
            assert_eq!(checked.config.peers.len(), 499);
            assert_eq!(checked.config.rules.len(), 998);
            (issue, start.elapsed(), res.body.len())
        })
        .buffer_unordered(2)
        .collect::<Vec<_>>()
        .await;
    let elapsed = start.elapsed();
    samples.sort_unstable_by_key(|s| s.0);
    let issue_p95 = samples[474].0;
    samples.sort_unstable_by_key(|s| s.1);
    let max_bytes = samples.iter().map(|s| s.2).max().unwrap();
    eprintln!(
        "500 renovaciones, concurrencia 2: total {elapsed:?}, HTTP p95 {issue_p95:?}, verificación p95 {:?}, máximo {max_bytes} bytes",
        samples[474].1
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM access_policies WHERE network_id=$1")
        .bind(net)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    t.finish().await;
}
