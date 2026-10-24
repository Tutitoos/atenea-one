//! Contrato de las rutas reales del control plane, construido con los tipos de utoipa.
use utoipa::openapi::{
    ComponentsBuilder, Content, Header, Info, OpenApi, OpenApiBuilder, Ref, RefOr, Required,
    Schema,
    path::{HttpMethod, OperationBuilder, ParameterBuilder, ParameterIn, PathItem, PathsBuilder},
    request_body::RequestBodyBuilder,
    response::ResponseBuilder,
    schema::{ArrayBuilder, ObjectBuilder, SchemaFormat, SchemaType, Type},
    security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme},
};

fn text() -> RefOr<Schema> {
    ObjectBuilder::new().schema_type(Type::String).into()
}

fn uuid() -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .format(Some(SchemaFormat::Custom("uuid".into())))
        .into()
}

fn timestamp() -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .format(Some(SchemaFormat::Custom("date-time".into())))
        .into()
}

fn nullable_timestamp() -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(SchemaType::from_iter([Type::String, Type::Null]))
        .format(Some(SchemaFormat::Custom("date-time".into())))
        .into()
}

fn integer() -> RefOr<Schema> {
    ObjectBuilder::new().schema_type(Type::Integer).into()
}

fn boolean() -> RefOr<Schema> {
    ObjectBuilder::new().schema_type(Type::Boolean).into()
}

fn nullable_text() -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(SchemaType::from_iter([Type::String, Type::Null]))
        .into()
}

fn named(name: &str) -> RefOr<Schema> {
    if let Some(item) = name.strip_prefix("[]") {
        ArrayBuilder::new()
            .items(Ref::from_schema_name(item))
            .into()
    } else {
        Ref::from_schema_name(name).into()
    }
}

fn object(fields: &[(&str, RefOr<Schema>)], required: &[&str]) -> RefOr<Schema> {
    let mut schema = ObjectBuilder::new().schema_type(Type::Object);
    for (name, field) in fields {
        schema = schema.property(*name, field.clone());
    }
    for name in required {
        schema = schema.required(*name);
    }
    schema.into()
}

fn complete(fields: &[(&str, RefOr<Schema>)]) -> RefOr<Schema> {
    object(
        fields,
        &fields.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    )
}

fn choices(values: &[&str]) -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .enum_values(Some(values.iter().copied()))
        .into()
}

fn hex(bytes: usize) -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .min_length(Some(bytes * 2))
        .max_length(Some(bytes * 2))
        .pattern(Some("^[0-9a-fA-F]+$"))
        .into()
}

fn schemas() -> ComponentsBuilder {
    let name: RefOr<Schema> = ObjectBuilder::new()
        .schema_type(Type::String)
        .min_length(Some(1))
        .max_length(Some(255))
        .description(Some(
            "Nombre no vacío ni compuesto solo por espacios; máximo 255 bytes UTF-8.",
        ))
        .into();
    let endpoints: RefOr<Schema> = ArrayBuilder::new()
        .items(text())
        .max_items(Some(4))
        .description(Some(
            "Direcciones IP:puerto; IPv6 entre corchetes. Puerto distinto de cero.",
        ))
        .into();
    let mut c = ComponentsBuilder::new();
    let definitions = [
        ("Error", complete(&[("error", complete(&[("code", text()), ("message", text())]))])),
        ("Role", choices(&["owner", "admin", "member"])),
        ("DeviceStatus", choices(&["pending", "approved", "revoked"])),
        ("Name", complete(&[("name", name.clone())])),
        ("OrganizationCreated", complete(&[("id", uuid()), ("name", text())])),
        ("Organization", complete(&[("id", uuid()), ("name", text()), ("role", named("Role"))])),
        ("Member", complete(&[("user_id", uuid()), ("role", named("Role"))])),
        ("SetMember", complete(&[("role", named("Role"))])),
        ("Empty", complete(&[])),
        ("User", complete(&[("id", uuid()), ("email", nullable_text()), ("display_name", nullable_text())])),
        ("Membership", complete(&[("org_id", uuid()), ("role", named("Role"))])),
        ("Session", complete(&[("user", named("User")), ("csrf_token", text()), ("memberships", named("[]Membership"))])),
        ("NewNetwork", complete(&[("name", name.clone()), ("ipv4_cidr", text()), ("ipv6_cidr", text()), ("root_anchor", hex(32))])),
        ("Network", complete(&[("id", uuid()), ("org_id", uuid()), ("name", text()), ("ipv4_cidr", text()), ("ipv6_cidr", text()), ("root_anchor", hex(32)), ("has_issuer_cert", boolean())])),
        ("NewEnrollmentToken", object(&[
            ("ttl_secs", ObjectBuilder::new().schema_type(Type::Integer).minimum(Some(60)).maximum(Some(86400)).into()),
            ("auto_approve", ObjectBuilder::new().schema_type(Type::Boolean).default(Some(serde_json::json!(false))).into()),
        ], &["ttl_secs"])),
        ("Join", complete(&[("url", text()), ("token", text()), ("network_id", uuid()), ("root_anchor", hex(32))])),
        ("EnrollmentToken", complete(&[("token", text()), ("expires_at", timestamp()), ("join", named("Join"))])),
        ("Enrollment", complete(&[("token", text()), ("bundle", hex(146)), ("name", name.clone())])),
        ("EnrolledDevice", complete(&[("device_id", uuid()), ("network_id", uuid()), ("status", choices(&["pending", "approved"]))])),
        ("Device", complete(&[("id", uuid()), ("device_id", uuid()), ("name", text()), ("status", named("DeviceStatus")), ("endpoints", endpoints.clone()), ("fingerprint", text()),("ipv4",nullable_text()),("ipv6",nullable_text()),("enrolled_at",timestamp()),("approved_at",nullable_timestamp()),("revoked_at",nullable_timestamp()),("last_config_at",nullable_timestamp()),("connection",named("Connection")),("report",utoipa::openapi::schema::OneOfBuilder::new().item(named("DeviceReport")).item(ObjectBuilder::new().schema_type(Type::Null)).into())])),
        ("DeviceState", complete(&[("device_id", uuid()), ("status", named("DeviceStatus"))])),
        ("DeviceId", complete(&[("device_id", uuid())])),
        ("DeviceUpdate", object(&[("name", name), ("endpoints", endpoints)], &[])),
        ("Id", complete(&[("id", uuid())])),
        ("Selector", object(&[("kind", choices(&["any", "device", "prefix"])), ("device_id", ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::String, Type::Null])).format(Some(SchemaFormat::Custom("uuid".into()))).into()), ("prefix", nullable_text())], &["kind"])),
        ("NewPolicy", object(&[
            ("src", named("Selector")), ("dst", named("Selector")),
            ("proto", ObjectBuilder::new().schema_type(Type::Integer).enum_values(Some([0, 1, 6, 17, 58])).into()),
            ("port_min", ObjectBuilder::new().schema_type(Type::Integer).minimum(Some(0)).maximum(Some(65535)).default(Some(serde_json::json!(0))).into()),
            ("port_max", ObjectBuilder::new().schema_type(Type::Integer).minimum(Some(0)).maximum(Some(65535)).default(Some(serde_json::json!(0))).into()),
        ], &["src", "dst", "proto"])),
        ("Policy", complete(&[("id", uuid()), ("src", named("Selector")), ("dst", named("Selector")), ("proto", integer()), ("port_min", integer()), ("port_max", integer())])),
        ("AuditEvent", complete(&[("id", integer()), ("at", timestamp()), ("actor_kind", choices(&["user","device","system"])), ("actor_id", text()), ("action", text()), ("target", text()), ("detail", ObjectBuilder::new().schema_type(Type::Object).into())])),
        ("SignedConfig", ObjectBuilder::new().schema_type(Type::String).format(Some(SchemaFormat::Custom("binary".into()))).description(Some("SignedConfig binario del contrato signed-config-v1; verificación con atenea-identity." )).into()),
        ("IssuerCertificate", ObjectBuilder::new().schema_type(Type::String).format(Some(SchemaFormat::Custom("binary".into()))).description(Some("IssuerCert binario de exactamente 178 bytes; firma raíz, red, emisor y vigencia comprobados." )).into()),
        ("OpenApiDocument", complete(&[("openapi", text()), ("info", ObjectBuilder::new().into()), ("paths", ObjectBuilder::new().into())])),
        ("Connection", choices(&["online","stale","offline","unknown"])),
        ("Path", choices(&["direct","relay-udp","relay-tls","probing","none"])),
        ("DevicePath", object(&[("peer",uuid()),("path",named("Path")),("last_rx_secs",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::Integer,Type::Null])).minimum(Some(0)).into()),("rtt_ms",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::Integer,Type::Null])).minimum(Some(0)).into()),("relay",nullable_text())], &["peer","path"])),
        ("NatMapping", ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::String,Type::Null])).enum_values(Some([serde_json::json!("independent"),serde_json::json!("dependent"),serde_json::json!("unknown"),serde_json::Value::Null])).into()),
        ("Route",complete(&[("id",uuid()),("device_id",uuid()),("prefix",text()),("status",choices(&["pending","approved","rejected"])),("snat",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::Boolean,Type::Null])).into()),("declared_at",timestamp()),("decided_at",nullable_timestamp())])),
        ("RouteDeclaration",complete(&[("routes",ArrayBuilder::new().items(text()).max_items(Some(8)).into())])),
        ("RouteApproval",complete(&[("snat",boolean())])),
        ("Dns",complete(&[("servers",ArrayBuilder::new().items(text()).max_items(Some(3)).into()),("domains",ArrayBuilder::new().items(text()).max_items(Some(8)).into())])),
        ("DnsState",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::String,Type::Null])).enum_values(Some([serde_json::json!("applied"),serde_json::json!("unsupported"),serde_json::json!("none"),serde_json::Value::Null])).into()),
        ("RouteReport",object(&[("prefix",text()),("state",choices(&["installed","conflict","pending"])),("reason",nullable_text())], &["prefix","state"])),
        ("RouteReports",utoipa::openapi::schema::OneOfBuilder::new().item(ArrayBuilder::new().items(named("RouteReport")).max_items(Some(256))).item(ObjectBuilder::new().schema_type(Type::Null)).into()),
        ("DeviceReportInput", object(&[("nat_mapping",named("NatMapping")),("paths",ArrayBuilder::new().items(named("DevicePath")).max_items(Some(256)).into()),("forwarding_ok",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::Boolean,Type::Null])).into()),("dns",named("DnsState")),("routes",named("RouteReports"))], &["paths"])),
        ("DeviceReport", complete(&[("reported_at",timestamp()),("age_secs",integer()),("nat_mapping",named("NatMapping")),("paths",named("[]DevicePath")),("forwarding_ok",ObjectBuilder::new().schema_type(SchemaType::from_iter([Type::Boolean,Type::Null])).into()),("dns",named("DnsState")),("routes",named("RouteReports"))])),
        ("NetworkSample",complete(&[("at",timestamp()),("total",integer()),("direct",integer()),("relay",integer()),("probing",integer()),("unknown",integer()),("offline",integer())])),
        ("NetworkEvent",object(&[("type",choices(&["hello","changed"])),("seq",integer()),("scope",choices(&["devices","policies","audit","network","metrics","routes","dns"]))], &["type","seq"])),
    ];
    for (name, schema) in definitions {
        c = c.schema(name, schema);
    }
    c.security_scheme("session", SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
        "atenea_session", "Sesión OIDC; HttpOnly, SameSite=Lax, Secure salvo desarrollo explícito. 8 h absolutas y 1 h de inactividad.",
    ))))
    .security_scheme("csrf", SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
        "X-CSRF-Token", "Token de GET /api/v1/session; obligatorio junto a la cookie en toda escritura de sesión.",
    ))))
    .security_scheme("device", SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
        "Authorization", "AtDevice v1 device=<hex16>, ts=<unix s>, nonce=<hex16>, sig=<hex64>. Firma Ed25519 sobre método, PATH_Y_QUERY, ts, nonce y BLAKE2s-256 del cuerpo con dominio atenea-one/api-auth/v1\\0. Ventana 60 s; nonce único durante 120 s.",
    ))))
}

/// Fuente única del contrato servido por HTTP y exportado con `--openapi`.
pub fn document() -> OpenApi {
    use HttpMethod::{Delete, Get, Patch, Post, Put};
    let mut paths = PathsBuilder::new();
    // Acceso: público, lectura/escritura de sesión o firma de dispositivo.
    // La matriz RBAC exige eliminar organizaciones aunque la tabla REST no enumere esa ruta.
    for (method, path, id, summary, access, status, body, result) in [
        (
            Get,
            "/healthz",
            "health",
            "Comprueba PostgreSQL",
            "public",
            "200",
            "",
            "",
        ),
        (
            Get,
            "/auth/login",
            "oidcLogin",
            "Inicia OIDC Authorization Code con PKCE S256",
            "public",
            "303",
            "",
            "",
        ),
        (
            Get,
            "/auth/callback",
            "oidcCallback",
            "Consume el flujo OIDC y crea una sesión",
            "public",
            "303",
            "",
            "",
        ),
        (
            Post,
            "/auth/logout",
            "logout",
            "Borra la sesión y su cookie",
            "write",
            "204",
            "",
            "",
        ),
        (
            Get,
            "/api/v1/session",
            "getSession",
            "Usuario, token CSRF y membresías",
            "read",
            "200",
            "",
            "Session",
        ),
        (
            Get,
            "/api/v1/orgs",
            "listOrganizations",
            "Lista las organizaciones del usuario",
            "read",
            "200",
            "",
            "[]Organization",
        ),
        (
            Post,
            "/api/v1/orgs",
            "createOrganization",
            "Crea una organización; el creador queda como owner",
            "write",
            "201",
            "Name",
            "OrganizationCreated",
        ),
        (
            Delete,
            "/api/v1/orgs/{org}",
            "deleteOrganization",
            "Elimina la organización y sus redes en cascada; requiere owner",
            "write",
            "204",
            "",
            "",
        ),
        (
            Get,
            "/api/v1/orgs/{org}/members",
            "listMembers",
            "Lista miembros; requiere pertenencia",
            "read",
            "200",
            "",
            "[]Member",
        ),
        (
            Put,
            "/api/v1/orgs/{org}/members/{user}",
            "setMember",
            "Añade un usuario registrado o cambia su rol; requiere owner",
            "write",
            "200",
            "SetMember",
            "Member",
        ),
        (
            Delete,
            "/api/v1/orgs/{org}/members/{user}",
            "removeMember",
            "Elimina una membresía; requiere owner y conserva el último owner",
            "write",
            "200",
            "",
            "Empty",
        ),
        (
            Get,
            "/api/v1/orgs/{org}/networks",
            "listNetworks",
            "Lista redes; requiere pertenencia",
            "read",
            "200",
            "",
            "[]Network",
        ),
        (
            Post,
            "/api/v1/orgs/{org}/networks",
            "createNetwork",
            "Crea una red sin CIDR solapados; requiere admin u owner",
            "write",
            "201",
            "NewNetwork",
            "Network",
        ),
        (
            Get,
            "/api/v1/networks/{net}",
            "getNetwork",
            "Obtiene una red; requiere pertenencia",
            "read",
            "200",
            "",
            "Network",
        ),
        (
            Delete,
            "/api/v1/networks/{net}",
            "deleteNetwork",
            "Elimina red y dispositivos en cascada; requiere admin u owner",
            "write",
            "204",
            "",
            "",
        ),
        (
            Put,
            "/api/v1/networks/{net}/issuer-cert",
            "setIssuerCertificate",
            "Valida y guarda el certificado del emisor; requiere admin u owner",
            "write",
            "204",
            "IssuerCertificate",
            "",
        ),
        (
            Post,
            "/api/v1/networks/{net}/enrollment-tokens",
            "createEnrollmentToken",
            "Crea un token de 256 bits, visible una sola vez; requiere admin u owner",
            "write",
            "201",
            "NewEnrollmentToken",
            "EnrollmentToken",
        ),
        (
            Get,
            "/api/v1/networks/{net}/devices",
            "listDevices",
            "Inventario con huellas para aprobación fuera de banda",
            "read",
            "200",
            "",
            "[]Device",
        ),
        (
            Post,
            "/api/v1/networks/{net}/devices/{dev}/approve",
            "approveDevice",
            "Aprueba y asigna IPv4/IPv6; requiere admin u owner",
            "write",
            "200",
            "",
            "DeviceState",
        ),
        (
            Post,
            "/api/v1/networks/{net}/devices/{dev}/revoke",
            "revokeDevice",
            "Revoca y libera direcciones; requiere admin u owner",
            "write",
            "200",
            "",
            "DeviceState",
        ),
        (
            Patch,
            "/api/v1/networks/{net}/devices/{dev}",
            "updateDevice",
            "Actualiza nombre o endpoints; requiere admin u owner",
            "write",
            "200",
            "DeviceUpdate",
            "DeviceId",
        ),
        (
            Get,
            "/api/v1/networks/{net}/policies",
            "listPolicies",
            "Lista ACL; requiere pertenencia",
            "read",
            "200",
            "",
            "[]Policy",
        ),
        (
            Post,
            "/api/v1/networks/{net}/policies",
            "createPolicy",
            "Valida y crea ACL; requiere admin u owner",
            "write",
            "201",
            "NewPolicy",
            "Id",
        ),
        (
            Delete,
            "/api/v1/networks/{net}/policies/{id}",
            "deletePolicy",
            "Elimina una ACL; requiere admin u owner",
            "write",
            "204",
            "",
            "",
        ),
        (
            Get,
            "/api/v1/networks/{net}/audit",
            "listAudit",
            "Auditoría de red ordenada por id descendente",
            "read",
            "200",
            "",
            "[]AuditEvent",
        ),
        (
            Post,
            "/api/v1/enroll",
            "enrollDevice",
            "Consume un token una sola vez y valida DeviceBundle",
            "public",
            "201",
            "Enrollment",
            "EnrolledDevice",
        ),
        (
            Get,
            "/api/v1/device/config",
            "getDeviceConfiguration",
            "Emite SignedConfig solo para un dispositivo aprobado",
            "device",
            "200",
            "",
            "SignedConfig",
        ),
        (
            Post,
            "/api/v1/device/routes",
            "declareRoutes",
            "Reemplaza las rutas declaradas; máximo una petición cada 5 s",
            "device",
            "204",
            "RouteDeclaration",
            "",
        ),
        (
            Get,
            "/api/v1/networks/{net}/routes",
            "listRoutes",
            "Lista las declaraciones y sus decisiones",
            "read",
            "200",
            "",
            "[]Route",
        ),
        (
            Post,
            "/api/v1/networks/{net}/routes/{id}/approve",
            "approveRoute",
            "Aprueba una ruta sin solapes; SNAT obligatorio",
            "write",
            "204",
            "RouteApproval",
            "",
        ),
        (
            Post,
            "/api/v1/networks/{net}/routes/{id}/reject",
            "rejectRoute",
            "Rechaza o revoca una ruta",
            "write",
            "204",
            "",
            "",
        ),
        (
            Get,
            "/api/v1/networks/{net}/dns",
            "getDns",
            "Obtiene DNS dividido de la red",
            "read",
            "200",
            "",
            "Dns",
        ),
        (
            Put,
            "/api/v1/networks/{net}/dns",
            "setDns",
            "Reemplaza servidores y dominios DNS",
            "write",
            "204",
            "Dns",
            "",
        ),
        (
            Post,
            "/api/v1/device/status",
            "reportDeviceStatus",
            "Reemplaza el informe declarado del dispositivo",
            "device",
            "204",
            "DeviceReportInput",
            "",
        ),
        (
            Get,
            "/api/v1/networks/{net}/events",
            "networkEvents",
            "Eventos WebSocket; Origin obligatorio, 8 conexiones por usuario y 64 por proceso",
            "read",
            "101",
            "",
            "NetworkEvent",
        ),
        (
            Get,
            "/api/v1/networks/{net}/metrics",
            "networkMetrics",
            "Muestras reales de los últimos siete días",
            "read",
            "200",
            "",
            "[]NetworkSample",
        ),
        (
            Get,
            "/api/openapi.json",
            "getOpenApi",
            "Obtiene este contrato OpenAPI",
            "public",
            "200",
            "",
            "OpenApiDocument",
        ),
    ] {
        let mut operation = OperationBuilder::new()
            .operation_id(Some(id))
            .summary(Some(summary))
            .tag(if path.starts_with("/auth") || id == "getSession" {
                "auth"
            } else if path.contains("policies") {
                "policies"
            } else if path.contains("devices")
                || id == "enrollDevice"
                || id == "createEnrollmentToken"
            {
                "devices"
            } else if path.contains("networks") {
                "networks"
            } else if path.contains("orgs") {
                "orgs"
            } else {
                "system"
            });
        if access == "read" || access == "write" {
            let security = if access == "write" {
                SecurityRequirement::new("session", Vec::<String>::new())
                    .add("csrf", Vec::<String>::new())
            } else {
                SecurityRequirement::new("session", Vec::<String>::new())
            };
            operation = operation.security(security);
        } else if access == "device" {
            operation =
                operation.security(SecurityRequirement::new("device", Vec::<String>::new()));
        }
        for part in path.split('/') {
            if let Some(name) = part
                .strip_prefix('{')
                .and_then(|part| part.strip_suffix('}'))
            {
                operation = operation.parameter(
                    ParameterBuilder::new()
                        .name(name)
                        .parameter_in(ParameterIn::Path)
                        .required(Required::True)
                        .schema(Some(uuid())),
                );
            }
        }
        if id == "oidcCallback" {
            for (name, required) in [("state", true), ("code", false), ("error", false)] {
                operation = operation.parameter(
                    ParameterBuilder::new()
                        .name(name)
                        .parameter_in(ParameterIn::Query)
                        .required(if required {
                            Required::True
                        } else {
                            Required::False
                        })
                        .schema(Some(text())),
                );
            }
            operation = operation.parameter(ParameterBuilder::new().name("atenea_oidc").parameter_in(ParameterIn::Cookie)
                .required(Required::True).schema(Some(text())).description(Some("State al azar fijado por login; debe coincidir con state en la query. Caduca a los 10 minutos y se consume una sola vez.")));
        }
        if id == "networkMetrics" {
            operation = operation
                .parameter(
                    ParameterBuilder::new()
                        .name("since")
                        .parameter_in(ParameterIn::Query)
                        .schema(Some(timestamp())),
                )
                .parameter(
                    ParameterBuilder::new()
                        .name("limit")
                        .parameter_in(ParameterIn::Query)
                        .schema(Some(
                            ObjectBuilder::new()
                                .schema_type(Type::Integer)
                                .minimum(Some(1))
                                .maximum(Some(1000))
                                .default(Some(serde_json::json!(360))),
                        )),
                );
        }
        if id == "listAudit" {
            operation = operation
                .parameter(
                    ParameterBuilder::new()
                        .name("limit")
                        .parameter_in(ParameterIn::Query)
                        .schema(Some(
                            ObjectBuilder::new()
                                .schema_type(Type::Integer)
                                .default(Some(serde_json::json!(100)))
                                .description(Some("Se acota al intervalo 1..500.")),
                        )),
                )
                .parameter(
                    ParameterBuilder::new()
                        .name("before")
                        .parameter_in(ParameterIn::Query)
                        .schema(Some(integer()))
                        .description(Some("Cursor exclusivo: devuelve eventos con id menor.")),
                );
        }
        if !body.is_empty() {
            let media = if body == "IssuerCertificate" {
                "application/octet-stream"
            } else {
                "application/json"
            };
            operation = operation.request_body(Some(
                RequestBodyBuilder::new()
                    .required(Some(Required::True))
                    .content(media, Content::new(Some(named(body))))
                    .build(),
            ));
        }
        let mut response = ResponseBuilder::new().description("Operación completada");
        if !result.is_empty() {
            response = response.content(
                if result == "SignedConfig" {
                    "application/octet-stream"
                } else {
                    "application/json"
                },
                Content::new(Some(named(result))),
            );
        }
        if id == "oidcLogin" || id == "oidcCallback" {
            response = response
                .header("Location", Header::new(text()))
                .header("Set-Cookie", Header::new(text()));
        }
        if id == "logout" {
            response = response.header("Set-Cookie", Header::new(text()));
        }
        if id == "getDeviceConfiguration" {
            response = response.header("X-Config-Version", Header::new(integer()));
        }
        if matches!(
            id,
            "oidcLogin" | "oidcCallback" | "logout" | "getSession" | "getDeviceConfiguration"
        ) {
            response = response.header(
                "Cache-Control",
                Header::new(
                    ObjectBuilder::new()
                        .schema_type(Type::String)
                        .enum_values(Some(["no-store"])),
                ),
            );
        }
        operation = operation.response(status, response);
        for (code, description) in [
            ("400", "Petición inválida"),
            (
                "401",
                "Sesión, flujo OIDC, token de enrolamiento o firma inválidos/caducados",
            ),
            (
                "403",
                "Permisos insuficientes, CSRF inválido o dispositivo no aprobado",
            ),
            (
                "404",
                "Recurso inexistente o ajeno a la organización del usuario",
            ),
            (
                "409",
                "Conflicto: último owner, duplicado, solapamiento, red llena o exceso de políticas/reglas",
            ),
            ("500", "Error interno sin detalles sensibles"),
        ] {
            if code == "500"
                || code == "400"
                || access != "public"
                || matches!(id, "oidcCallback" | "enrollDevice")
            {
                operation = operation.response(
                    code,
                    ResponseBuilder::new()
                        .description(description)
                        .content("application/json", Content::new(Some(named("Error")))),
                );
            }
        }
        if matches!(
            id,
            "enrollDevice" | "reportDeviceStatus" | "networkEvents" | "declareRoutes"
        ) {
            operation = operation.response(
                "429",
                ResponseBuilder::new()
                    .description("Límite de peticiones o conexiones excedido")
                    .content("application/json", Content::new(Some(named("Error")))),
            );
        }
        if id == "getDeviceConfiguration" {
            operation = operation.response("503", ResponseBuilder::new().description("issuer_unavailable: certificado ausente, caducado o insuficiente para el lease").content("application/json", Content::new(Some(named("Error")))));
        }
        if id == "oidcLogin" || id == "oidcCallback" {
            operation = operation.response(
                "502",
                ResponseBuilder::new()
                    .description("Proveedor OIDC no disponible")
                    .content("application/json", Content::new(Some(named("Error")))),
            );
        }
        if id == "createPolicy" {
            operation = operation.description(Some("Selectores: any no admite campos extra; device exige device_id de esta red; prefix exige un CIDR canónico contenido en la red o en una ruta aprobada. Una ruta retirada deja su política huérfana sin seleccionar pares. Solo TCP/UDP admite puertos; port_min ≤ port_max. Máximo 256 políticas y 256 reglas efectivas contando inversas sin puertos y eliminando duplicados."));
        }
        if id == "getDeviceConfiguration" {
            operation = operation.description(Some("Respuesta firmada con versión estrictamente creciente. Sin políticas no hay pares. Excluye dispositivos pendientes/revocados. Incluye reglas inversas sin puertos. not_after no supera el certificado del emisor; duración por defecto 300 s."));
        }
        paths = paths.path(path, PathItem::new(method, operation));
    }
    OpenApiBuilder::new()
        .info(Info::new("Atenea One control plane", "1.0.0"))
        .paths(paths.build())
        .components(Some(schemas().build()))
        .build()
}

#[cfg(test)]
mod tests {
    #[test]
    fn contract_covers_routes_and_security() {
        let document = serde_json::to_value(super::document()).unwrap();
        assert_eq!(document["paths"].as_object().unwrap().len(), 31);
        assert_eq!(
            document["paths"]["/api/v1/session"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/Session"
        );
        assert!(
            document["paths"]["/auth/logout"]["post"]["security"][0]
                .get("csrf")
                .is_some()
        );
        assert!(
            document["paths"]["/api/v1/device/config"]["get"]["responses"]["200"]["content"]
                .get("application/octet-stream")
                .is_some()
        );
        assert_eq!(
            document["paths"]["/api/v1/device/status"]["post"]["requestBody"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/DeviceReportInput"
        );
        assert!(
            document["components"]["schemas"]["Device"]["properties"]
                .get("connection")
                .is_some()
        );
    }
}
