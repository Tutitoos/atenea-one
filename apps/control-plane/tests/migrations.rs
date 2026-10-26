// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use common::TestApp;
use serde_json::Value;
use sqlx::{Executor, Row};

fn assert_allocation_index(plan: &Value) -> usize {
    let mut scans = 0;
    if plan["Relation Name"] == "ip_allocations" {
        assert!(
            matches!(
                plan["Node Type"].as_str(),
                Some("Index Only Scan" | "Index Scan")
            ),
            "barrido global de asignaciones: {plan}"
        );
        assert_eq!(plan["Index Name"], "ip_allocations_device_family");
        scans += 1;
    }
    if let Some(children) = plan["Plans"].as_array() {
        for child in children {
            scans += assert_allocation_index(child);
        }
    }
    scans
}

#[tokio::test]
async fn migracion_indice_asignaciones_cubre_emision_y_listado_e_idempotencia() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    // 10k dispositivos repartidos en 79 redes: 128 por red y 20k asignaciones.
    // Los bytes sintéticos satisfacen SQL; no son identidades criptográficas.
    t.pool.execute("INSERT INTO organizations(id,name) VALUES (md5('org')::uuid,'index-test');
        INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor)
        SELECT md5('net-' || i)::uuid,md5('org')::uuid,'net-' || i,'10.90.0.0/16','fd90::/64',decode(repeat('00',32),'hex') FROM generate_series(0,78) i;
        INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle)
        SELECT md5(i::text)::uuid,md5('net-' || ((i-1)/128))::uuid,'device-' || i,'approved',decode(md5(i::text) || md5(i::text),'hex'),decode(md5(i::text) || md5(i::text),'hex'),decode(repeat('00',146),'hex') FROM generate_series(1,10000) i;
        INSERT INTO ip_allocations(network_id,device_id,family,addr)
        SELECT md5('net-' || ((i-1)/128))::uuid,md5(i::text)::uuid,4,'10.90.0.0'::inet+i FROM generate_series(1,10000) i;
        INSERT INTO ip_allocations(network_id,device_id,family,addr)
        SELECT md5('net-' || ((i-1)/128))::uuid,md5(i::text)::uuid,6,'fd90::'::inet+i FROM generate_series(1,10000) i;
        ANALYZE ip_allocations; ANALYZE devices; ANALYZE device_reports;")
        .await.unwrap();
    // Como en el A/B del diagnóstico, fijar visibilidad tras cargar la fixture sintética.
    t.pool
        .execute("VACUUM ANALYZE ip_allocations")
        .await
        .unwrap();

    // SQL de issue::load_devices y devices::list, sin forzar enable_seqscan.
    for (query, scans) in [
        (
            "EXPLAIN (FORMAT JSON) SELECT host(addr) FROM ip_allocations WHERE device_id=md5('1')::uuid ORDER BY family",
            1,
        ),
        (
            "EXPLAIN (FORMAT JSON) SELECT d.id,d.name,d.status,d.bundle,d.endpoints,d.enrolled_at,d.approved_at,d.revoked_at,d.last_config_at,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=4) ipv4,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=6) ipv6,r.reported_at,r.nat_mapping,r.paths,r.forwarding_ok,r.dns,r.routes,GREATEST(0,floor(extract(epoch FROM now()-r.reported_at))::bigint) age_secs FROM devices d LEFT JOIN device_reports r ON r.device_id=d.id WHERE d.network_id=md5('net-0')::uuid ORDER BY d.id",
            2,
        ),
    ] {
        let row = sqlx::query(query).fetch_one(&t.pool).await.unwrap();
        let plan: Value = row.get(0);
        assert_eq!(assert_allocation_index(&plan[0]["Plan"]), scans);
    }

    let definition: String = sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname='public' AND indexname='ip_allocations_device_family'")
        .fetch_one(&t.pool).await.unwrap();
    assert_eq!(
        definition,
        "CREATE INDEX ip_allocations_device_family ON public.ip_allocations USING btree (device_id, family) INCLUDE (addr)"
    );
    // TestApp ya ha migrado: una segunda pasada conserva índice, datos e historial.
    atenea_control_plane::db::migrate(&t.pool, None)
        .await
        .unwrap();
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
            .fetch_all(&t.pool)
            .await
            .unwrap();
    assert_eq!(versions, vec![1, 2, 3, 4, 5]);
    let allocations: i64 = sqlx::query_scalar("SELECT count(*) FROM ip_allocations")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(allocations, 20000);
    t.finish().await;
}
