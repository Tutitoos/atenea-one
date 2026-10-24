global
    maxconn 128

defaults
    mode tcp
    timeout connect 3s
    timeout client 10s
    timeout server 10s

frontend entrada_443
    bind 0.0.0.0:443
    bind [::]:443 v6only
    tcp-request inspect-delay 5s
    tcp-request content accept if { req.ssl_hello_type 1 }
    use_backend relay if { req.ssl_sni -i relay.atenea.one } { req.ssl_alpn -m str atenea-relay/1 }
    default_backend web

backend relay
    server relay 127.0.0.1:@RELAY_PORT@ send-proxy-v2

backend web
    server web 127.0.0.1:@WEB_PORT@
