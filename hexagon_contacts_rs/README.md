# Hexagonal Architecture Demo (Rust): Kundenkontakte

1:1-Übertragung des Python-Beispiels nach Rust. Traits übernehmen die
Rolle der Ports (statt ABCs), Structs die Rolle der Adapter.

## Struktur

```
hexagon_contacts_rs/
├── src/
│   ├── domain/
│   │   ├── entity.rs      # Contact - reines Struct, keine Abhängigkeiten
│   │   ├── error.rs       # DomainError - geteilter Fehlertyp beider Ports
│   │   └── service.rs     # ContactService - Geschäftslogik (Validierung etc.)
│   ├── port/
│   │   ├── inbound/
│   │   │   └── contact_port.rs       # ContactUseCase-Trait
│   │   └── outbound/
│   │       └── contact_repository.rs # ContactRepository-Trait
│   ├── adapter/
│   │   ├── inbound/odata/
│   │   │   └── contact_odata_adapter.rs  # axum-Router im OData-V2-Stil
│   │   └── outbound/postgres/
│   │       └── contact_repository.rs     # tokio-postgres-Implementierung
│   ├── lib.rs
│   └── main.rs            # Composition Root
├── tests/
│   └── domain_decoupling.rs  # Beweis: Domain testbar ohne echtes Postgres
└── Cargo.toml
```

## Rust-spezifische Unterschiede zu Python

- **Trait statt ABC**: `#[async_trait]` wird gebraucht, weil native `async fn`
  in Traits (Stand Rust 2021/dieser Toolchain) noch nicht objektsicher
  (`dyn`-fähig) sind.
- **`Arc<dyn Trait>` statt Duck-Typing**: `ContactService` hält
  `Arc<dyn ContactRepository>` - das ist die Entsprechung dazu, dass der
  Python-Code einfach ein beliebiges Objekt mit den richtigen Methoden
  akzeptiert. In Rust muss die Trait-Grenze explizit sein.
- **Kein Duck-Typing bei Fehlern**: `DomainError` ist ein eigenes Enum, das
  beide Seiten (Domain und Adapter) teilen - kommt dem Python-`ValueError`/
  `LookupError`-Pattern am nächsten, ist aber explizit typisiert.
- **Compile-Time-Beweis der Entkopplung**: `tests/domain_decoupling.rs`
  bringt eine `InMemoryContactRepository` mit, die `ContactRepository`
  implementiert - der Test läuft ganz ohne Postgres-Verbindung.

## Ausführen

```bash
export CONTACTS_DSN="host=localhost user=postgres password=postgres dbname=contacts"
cargo run
```

## Tests

```bash
cargo test
```

## Beispielaufrufe

Identisch zum Python-Beispiel (gleiche Routen, gleiches JSON-Format):

```bash
curl -X POST http://localhost:5000/odata/v2/ContactService.svc/Contacts \
  -H "Content-Type: application/json" \
  -d '{"CustomerID": "K123", "Name": "Max Mustermann", "Email": "max@example.com"}'

curl "http://localhost:5000/odata/v2/ContactService.svc/Contacts?\$filter=CustomerID%20eq%20%27K123%27"

curl http://localhost:5000/odata/v2/ContactService.svc/Contacts/1

curl -X DELETE http://localhost:5000/odata/v2/ContactService.svc/Contacts/1
```

## Hinweis zu den Dependency-Versionen

`tokio-postgres`, `postgres-types` und `postgres-protocol` sind in
`Cargo.toml`/`Cargo.lock` bewusst auf ältere Versionen gepinnt
(0.7.7 / 0.2.6 / 0.6.6). Neuere Releases ziehen `sha2 0.11` und damit
`cpufeatures 0.3` nach, was Cargos `edition2024`-Feature voraussetzt -
das lief auf der hier verwendeten Toolchain (Rust 1.75, aus dem
Ubuntu-Repository) noch nicht. Mit einer aktuellen Rust-Toolchain
(1.85+, z. B. via `rustup`) können die Versionsgrenzen in `Cargo.toml`
wieder entfernt werden.
