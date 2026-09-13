Parser-Projekt – Architektur- und Portierungsplan
1. Ziel

Entwicklung eines Parsers für ein eigenes Dateiformat mit möglichst breiter Verwendbarkeit.

Zielplattformen:

Rust
C
Python
JavaScript / TypeScript (Node.js)

Dabei soll es eine vollständige, moderne Referenzimplementierung geben und zusätzlich eine möglichst kleine, unabhängige C-Implementierung für Umgebungen, in denen Rust nicht eingesetzt werden kann.

2. Grundarchitektur
                         Dateiformat-Spezifikation
                                   │
                                   ▼
                         Gemeinsame Test-Suite
                                   │
                    ┌──────────────┴──────────────┐
                    │                             │
                    ▼                             ▼
             Rust-Implementierung          C Single-Header
               (Referenz-Core)             (Small / Portable)
                    │
          ┌─────────┴─────────┐
          │                   │
          ▼                   ▼
       Python             Node.js / TS
       PyO3                napi-rs
          │                   │
          ▼                   ▼
        PyPI                 npm


Wichtig:

Rust und C sind zwei Implementierungen desselben Formats.

Python und Node.js sollen dagegen nicht eigene Parser enthalten, sondern auf der Rust-Implementierung aufbauen.

3. Rust – primäre Referenzimplementierung

Rust ist die zentrale Implementierung für die vollständige Funktionalität.

Aufgaben:

Parsing
Validierung
Fehlerbehandlung
Datenmodell
Serialisierung, falls vorgesehen
Streaming, falls vorgesehen
Versions-/Kompatibilitätslogik
Security-relevante Prüfungen

Beispielhafte API:

pub fn parse(data: &[u8]) -> Result<Document, Error>;

pub fn parse_reader<R: Read>(reader: R) -> Result<Document, Error>;

pub fn validate(data: &[u8]) -> Result<(), Error>;


Die öffentliche Rust-API soll idiomatisch Rust sein und nicht einfach eine C-API nachbilden.

Package:

crates.io


Beispiel:

use myformat::parse;

let document = parse(&data)?;

4. C – Single-Header-Implementation

Zusätzlich entsteht eine eigenständige C-Implementierung für kleine und restriktive Umgebungen.

Ziel:

Single Header
möglichst keine Dependencies
C99 oder klar definierter Standard
kleine Binary-/Memory-Footprint
Embedded-freundlich
einfache Integration
keine Heap-Nutzung, wenn optional vermeidbar
keine Threads erforderlich
kein Betriebssystem erforderlich

Typisches Integrationsmodell:

#define MYFORMAT_IMPLEMENTATION
#include "myformat.h"


oder:

#include "myformat.h"

myformat_document_t doc;

myformat_parse(data, size, &doc);


Die C-Version muss nicht intern genauso aufgebaut sein wie die Rust-Version.

Entscheidend ist:

Beide Implementierungen müssen dieselbe Spezifikation erfüllen.

5. Gemeinsame Test-Suite

Die gemeinsame Testsuite ist ein zentraler Bestandteil des Projekts.

Sie verhindert, dass Rust und C im Laufe der Zeit unterschiedliche Interpretationen des Formats entwickeln.

Beispiel:

tests/
├── valid/
│   ├── minimal.dat
│   ├── example1.dat
│   └── example2.dat
│
├── invalid/
│   ├── truncated.dat
│   ├── invalid_header.dat
│   └── invalid_length.dat
│
├── edge/
│   ├── empty.dat
│   ├── max_values.dat
│   └── large.dat
│
└── expected/
    ├── minimal.json
    └── example1.json


Jede Implementierung muss dieselben Testdateien verarbeiten.

Ideal ist ein automatischer Cross-Implementation-Test:

                    test vectors
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
          Rust parser           C parser
              │                     │
              └──────────┬──────────┘
                         ▼
                    gleiche
                    Ergebnisse

6. Python-Binding

Python soll direkt an die Rust-Implementierung angebunden werden.

Technologie:

PyO3
maturin

Architektur:

Python
   │
   ▼
PyO3 binding
   │
   ▼
Rust parser


Python-API soll Python-typisch sein:

import myformat

doc = myformat.parse(data)

print(doc.header)
print(doc.entries)


Nicht:

myformat.myformat_parse(ptr, size, ...)


Die Python-Schicht soll Rust-Fehler in geeignete Python-Exceptions übersetzen.

Packaging:

PyPI


Ziel:

pip install myformat

7. JavaScript / TypeScript

Die JavaScript-Seite sollte als TypeScript-first API entwickelt werden.

Der eigentliche native Parser bleibt Rust.

Technologie:

napi-rs
Node.js native addon
TypeScript API / Typdefinitionen

Architektur:

TypeScript
    │
    ▼
TypeScript API
    │
    ▼
napi-rs
    │
    ▼
Rust parser


Beispiel:

import { parse } from "@myformat/node";

const document = parse(data);

console.log(document.header);


Das Package soll sowohl von JavaScript als auch TypeScript problemlos verwendet werden können.

Packaging:

npm


Beispiel:

npm install @myformat/node

8. Warum TypeScript statt einer separaten JavaScript-Implementierung?

Es soll kein zweiter Parser in JavaScript entstehen.

Stattdessen:

Rust parser
     │
     ▼
native Node module
     │
     ▼
TypeScript API
     │
     ├── TypeScript
     └── JavaScript


Damit erhalten JavaScript-Nutzer ebenfalls die Library, während TypeScript-Nutzer zusätzlich eine starke Typisierung bekommen.

9. C-ABI für zukünftige Bindings

Optional sollte der Rust-Core eine kleine stabile C-kompatible ABI erhalten.

Beispielsweise:

myformat_document_t *
myformat_parse(const uint8_t *data, size_t len);

void
myformat_document_free(myformat_document_t *document);


Damit können später weitere Sprachen angebunden werden:

Rust
 │
 └── C ABI
      ├── C/C++
      ├── C#
      ├── Lua
      ├── Ruby
      └── weitere FFI-Systeme


Die C-ABI sollte bewusst klein gehalten werden.

10. Was NICHT gemacht werden sollte

Keine sieben unabhängigen Parser schreiben.

Insbesondere nicht:

C parser
Rust parser
Python parser
JS parser
Go parser
Lua parser
Perl parser


Das führt langfristig zu unterschiedlichen Bugs und unterschiedlichem Verhalten.

Stattdessen:

Rust = vollständige moderne Implementierung

C = kleine unabhängige Implementierung

Python = Binding
JS/TS = Binding

11. Erweiterung auf weitere Sprachen

Weitere Sprachen erst hinzufügen, wenn tatsächlicher Bedarf besteht.

Priorität:

1. Rust
2. C
3. Python
4. TypeScript / Node.js
5. Go
6. Lua
7. weitere Sprachen


Go kann später beispielsweise über die C-ABI oder eine direkte Rust-Anbindung integriert werden.

Lua ist besonders interessant, falls das Format in Spielen, Embedded-Systemen oder Plugin-Systemen verwendet wird.

12. Format-Spezifikation

Die Spezifikation sollte unabhängig von einer konkreten Implementierung sein.

Sie sollte mindestens definieren:

Magic Number / File Signature
Versionierung
Header
Datentypen
Byte Order / Endianness
Größenfelder
Offsets
Alignment
optionale Felder
unbekannte Felder
Fehlerbedingungen
maximale Größen
Kompatibilitätsregeln
Canonical Representation, falls vorhanden
Security-Anforderungen

Beispiel:

File
 ├── Header
 │    ├── Magic
 │    ├── Version
 │    └── Flags
 │
 ├── Metadata
 │
 └── Entries
      ├── Entry
      ├── Entry
      └── ...

13. Fehlerbehandlung

Fehler müssen Bestandteil der Spezifikation sein.

Beispiele:

InvalidMagic
UnsupportedVersion
UnexpectedEOF
InvalidLength
InvalidOffset
IntegerOverflow
InvalidEncoding
InvalidEntry


Rust:

Result<T, Error>


Python:

try:
    document = myformat.parse(data)
except myformat.ParseError:
    ...


C:

myformat_error_t error;

if (myformat_parse(...) != MYFORMAT_OK) {
    ...
}

14. Security

Da Parser häufig untrusted Input verarbeiten, sollte Security von Anfang an berücksichtigt werden.

Insbesondere:

Integer Overflow
Integer Truncation
Out-of-bounds Reads
Out-of-bounds Writes
ungültige Offsets
zyklische Referenzen
übergroße Längenangaben
Memory Exhaustion
Stack Exhaustion
extrem tiefe Verschachtelung
DoS durch absichtlich ungünstige Dateien

Die Rust-Version sollte mit Fuzzing getestet werden.

Für C ebenfalls:

AddressSanitizer
UndefinedBehaviorSanitizer
Fuzzing
Compiler-Warnings auf höchstem sinnvollen Niveau
15. Fuzzing

Fuzzing sollte ein fester Bestandteil der Entwicklung sein.

Beispiel:

                 Fuzzer
                   │
                   ▼
              random input
                   │
          ┌────────┴────────┐
          ▼                 ▼
       Rust parser       C parser
          │                 │
          └────────┬────────┘
                   ▼
              comparison


Besonders interessant:

Parser darf niemals abstürzen
keine Panics bei untrusted Input
keine Undefined Behavior
C und Rust sollten bei identischen Inputs konsistent reagieren
16. CI

Jeder Commit sollte mindestens testen:

Rust
├── unit tests
├── integration tests
├── fuzz smoke tests
└── clippy

C
├── compiler matrix
├── unit tests
├── sanitizers
└── strict warnings

Python
└── binding/integration tests

Node/TS
├── native module tests
└── TypeScript tests


Zusätzlich:

Shared test vectors
        │
   ┌────┴────┐
   ▼         ▼
 Rust       C

17. Release-Struktur

Mögliche Repository-Struktur:

myformat/
├── spec/
│   ├── FORMAT.md
│   └── examples/
│
├── testdata/
│   ├── valid/
│   ├── invalid/
│   └── expected/
│
├── rust/
│   ├── parser/
│   └── bindings/
│
├── c/
│   └── myformat.h
│
├── python/
│   └── ...
│
├── node/
│   └── ...
│
├── fuzz/
│   └── ...
│
├── tools/
│   └── ...
│
├── tests/
│   └── ...
│
├── LICENSE
├── README.md
└── Cargo.toml


Je nach Projektgröße können die Rust-/Python-/Node-Strukturen natürlich als Cargo-Workspace organisiert werden.

18. CLI-Tool

Zusätzlich zu den Libraries sollte ein CLI-Tool entwickelt werden.

Beispielsweise:

myformat info file.dat
myformat validate file.dat
myformat dump file.dat
myformat convert file.dat output.dat


Das CLI basiert auf dem Rust-Core.

Das ist wichtig, weil Entwickler damit das Format untersuchen können, ohne selbst eine Library einzubauen.

19. Dokumentation

Mindestens:

README
Format specification
Getting started
Rust API
C API
Python API
Node/TypeScript API
Examples
Security considerations
Compatibility / versioning
FAQ


Besonders wertvoll sind kleine Beispiele.

Zum Beispiel:

"How do I read a file?"
"How do I validate a file?"
"How do I create a file?"
"How do I stream a file?"
"How do I inspect a file?"

20. Empfohlene Reihenfolge
Phase 1 – Spezifikation
Dateiformat vollständig definieren
Versionierung festlegen
Fehlerfälle definieren
Testdateien erstellen
Phase 2 – Rust
Datenmodell
Parser
Validierung
Serializer, falls erforderlich
Unit Tests
Integration Tests
Fuzzing
Phase 3 – C
Single-Header-Parser
möglichst geringe Abhängigkeiten
C-Test-Suite
Sanitizer
Cross-Tests gegen Rust
Phase 4 – Python
PyO3
maturin
Python API
PyPI Package
Integration Tests
Phase 5 – Node/TypeScript
napi-rs
TypeScript API
npm Package
Node.js Integration Tests
Phase 6 – CLI
inspect
validate
dump
convert
Phase 7 – Release
Dokumentation
Beispiele
Testdaten
CI
Security Review
erste stabile Formatversion
21. Endziel

Das Projekt sollte sich für den Anwender ungefähr so anfühlen:

Rust
myformat = "1.x"

let doc = myformat::parse(data)?;

Python
pip install myformat

import myformat

doc = myformat.parse(data)

Node / TypeScript
npm install @myformat/node

import { parse } from "@myformat/node";

const doc = parse(data);

C
myformat.h

#define MYFORMAT_IMPLEMENTATION
#include "myformat.h"


Damit gibt es eine moderne, sichere und komfortable Library für große Anwendungen sowie eine extrem portable C-Version für kleine Systeme.

22. Grundprinzip

Die wichtigste Architekturentscheidung lautet:

Die Spezifikation ist die gemeinsame Wahrheit. Rust ist die primäre vollständige Implementierung. C ist die bewusst kleine, portable Alternative. Python und Node/TypeScript sind Bindings zum Rust-Core.

Dadurch bleibt die Anzahl der tatsächlich zu pflegenden Parser gering, während trotzdem ein sehr breites Ökosystem abgedeckt wird.