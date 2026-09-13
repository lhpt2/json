Parser-Projekt – Architektur, Bindings und gemeinsame Tests
1. Ziel

Entwicklung eines Parsers für ein eigenes Dateiformat mit möglichst breiter Verwendbarkeit.

Geplante Zielplattformen:

Rust
C
Python
JavaScript / TypeScript auf Node.js

Die Architektur soll dabei zwei unterschiedliche Anforderungen erfüllen:

Eine moderne, sichere und komfortable Implementierung für die meisten Anwendungen.
Eine möglichst kleine, portable C-Implementierung für kleine Umgebungen und Embedded-Systeme.

Die zentrale Idee lautet:

Rust ist die primäre vollständige Implementierung. C ist eine unabhängige, portable Single-Header-Implementierung. Python und Node.js/TypeScript werden an den Rust-Core angebunden.

2. Warum diese Sprachen?

Die ursprünglich diskutierten Sprachen waren:

C
Python
Lua
Go
Rust
Perl
JavaScript / TypeScript

Für eine möglichst breite Verbreitung wurde folgende Priorisierung gewählt:

C
Rust
Python
JavaScript / TypeScript
Go
Lua
Perl

Für das konkrete Projekt wird zunächst nur Folgendes umgesetzt:

Rust
C
Python
TypeScript / Node.js


Weitere Sprachen können später ergänzt werden.

3. Grundarchitektur
                         Dateiformat-Spezifikation
                                   │
                                   ▼
                         Gemeinsame Test-Suite
                                   │
                    ┌──────────────┴──────────────┐
                    │                             │
                    ▼                             ▼
             Rust-Implementierung          C Single-Header
               (vollständig)               (portable / small)
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

Der Rust-Core kennt Python und Node.js nicht.
Der C-Parser ist unabhängig vom Rust-Code.
Python und Node.js enthalten keinen zweiten Parser.
Die Spezifikation und die Testdaten sind die gemeinsame Wahrheit.
4. Rust als primäre Implementierung

Rust übernimmt die vollständige moderne Implementierung des Formats.

Dazu gehören beispielsweise:

Parsing
Validierung
Datenmodell
Fehlerbehandlung
Serialisierung
Streaming
Versionsverwaltung
Kompatibilitätslogik
Security-relevante Prüfungen

Beispielhafte API:

pub fn parse(data: &[u8]) -> Result<Document, Error>;

pub fn parse_reader<R: Read>(reader: R) -> Result<Document, Error>;

pub fn validate(data: &[u8]) -> Result<(), Error>;


Die Rust-API sollte idiomatisch für Rust gestaltet werden.

Beispiel:

let document = myformat::parse(&data)?;


Nicht das Design der späteren C-, Python- oder JavaScript-API sollte die Rust-API bestimmen.

5. C Single-Header

Zusätzlich wird eine eigenständige C-Implementierung erstellt.

Ziel:

Single Header
möglichst keine externen Dependencies
C99 oder ein klar definierter Standard
kleine Binary-Größe
geringer Speicherbedarf
Embedded-freundlich
keine Betriebssystemabhängigkeit
optional ohne Heap-Nutzung
einfache Integration

Typisches Single-Header-Modell:

#define MYFORMAT_IMPLEMENTATION
#include "myformat.h"


oder:

#include "myformat.h"

myformat_document_t doc;

myformat_parse(data, size, &doc);


Die C-Implementierung muss nicht intern genauso aufgebaut sein wie die Rust-Implementierung.

Sie muss aber dieselbe Formatspezifikation erfüllen.

6. Warum C und Rust beide?

Rust und C erfüllen unterschiedliche Aufgaben.

Rust

Rust ist die komfortable, moderne und sicherheitsorientierte Implementierung.

Sie ist geeignet für:

Server
Desktop
CLI-Tools
größere Anwendungen
komplexere Parserlogik
sichere Verarbeitung von untrusted Input
C

C ist die minimalistische, portable Variante.

Sie ist geeignet für:

Embedded
kleine Systeme
Bootloader-nahe Umgebungen
bestehende C/C++-Software
Umgebungen ohne Rust-Toolchain
sehr kleine Deployments

Dadurch muss die Rust-Implementierung nicht künstlich auf die Einschränkungen eines Single-Headers reduziert werden.

7. Python-Binding

Python wird direkt an den Rust-Core angebunden.

Verwendete Technologien:

PyO3
maturin

Architektur:

Python
   │
   ▼
PyO3
   │
   ▼
Rust Core


Beispiel:

import myformat

document = myformat.parse(data)

print(document.header)


Die Python-API soll Python-typisch gestaltet werden.

Rust-Fehler werden in geeignete Python-Exceptions übersetzt.

Beispielsweise:

try:
    document = myformat.parse(data)
except myformat.ParseError:
    ...


Packaging erfolgt über PyPI.

Ziel:

pip install myformat

8. Node.js / TypeScript

Für Node.js wird ebenfalls der Rust-Core verwendet.

Technologie:

napi-rs
Node.js N-API
TypeScript

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
Rust Core


Beispiel:

import { parse } from "@myformat/node";

const document = parse(data);

console.log(document.header);


Das Package soll sowohl JavaScript- als auch TypeScript-Nutzer unterstützen.

TypeScript wird dabei als TypeScript-first API behandelt.

Es muss keine separate JavaScript-Parserimplementierung geben.

9. Warum TypeScript statt JavaScript?

Die native Implementierung befindet sich in Rust.

JavaScript und TypeScript greifen auf dasselbe native Node-Modul zu.

Rust
 │
 ▼
native Node module
 │
 ▼
TypeScript API
 │
 ├── TypeScript
 └── JavaScript


Damit bekommt TypeScript starke Typisierung, während normale JavaScript-Projekte das Package ebenfalls verwenden können.

10. Optionale C-ABI des Rust-Cores

Zusätzlich kann der Rust-Core später eine kleine C-kompatible ABI anbieten.

Beispielsweise:

myformat_document_t *
myformat_parse(const uint8_t *data, size_t len);

void
myformat_document_free(myformat_document_t *document);


Damit können später weitere Sprachen relativ einfach angebunden werden:

Rust Core
   │
   └── C ABI
        ├── C / C++
        ├── C#
        ├── Lua
        ├── Ruby
        ├── Go
        └── weitere FFI-Systeme


Die C-ABI sollte bewusst klein bleiben.

11. Gemeinsame Test-Suite

Die gemeinsame Testsuite ist ein zentraler Bestandteil.

Die Testdaten gehören weder zu Rust noch zu C.

Sie gehören zum Dateiformat selbst.

Empfohlene Struktur:

testdata/
├── valid/
│   ├── minimal.dat
│   ├── example1.dat
│   └── example2.dat
│
├── invalid/
│   ├── truncated.dat
│   ├── bad-magic.dat
│   └── bad-version.dat
│
└── expected/
    ├── minimal.json
    ├── example1.json
    └── example2.json


Die gleiche Datei wird von allen Implementierungen getestet.

12. Testvektoren

Ein Testfall sollte sowohl eine Eingabedatei als auch ein erwartetes Ergebnis definieren.

Beispiel:

{
  "file": "minimal.dat",
  "valid": true,
  "version": 1,
  "flags": 0,
  "entry_count": 0
}


Für eine ungültige Datei:

{
  "file": "bad-magic.dat",
  "valid": false,
  "error": "InvalidMagic"
}


Dadurch wird nicht nur getestet:

Kann der Parser diese Datei lesen?

sondern auch:

Wird eine fehlerhafte Datei korrekt erkannt?

13. Golden Files

Für komplexere Formate eignen sich Golden Files.

Beispiel:

testdata/
├── valid/
│   ├── simple.dat
│   ├── metadata.dat
│   ├── unicode.dat
│   └── large.dat
│
└── expected/
    ├── simple.json
    ├── metadata.json
    ├── unicode.json
    └── large.json


Die Parser lesen die .dat-Dateien und liefern eine semantische Darstellung.

Diese wird mit dem erwarteten Ergebnis verglichen.

Damit können auch sehr komplexe Dateien dauerhaft als Referenz dienen.

14. Tests für Rust

Rust kann die Testdaten direkt einlesen.

Beispiel:

#[test]
fn test_minimal() {
    let data = std::fs::read("../testdata/valid/minimal.dat").unwrap();

    let document = myformat_core::parse(&data).unwrap();

    assert_eq!(document.version, 1);
    assert_eq!(document.flags, 0);
}


Bei vielen Testfällen sollte nicht für jede Datei ein eigener Test geschrieben werden.

Besser ist ein Test-Runner, der automatisch alle Testfälle durchläuft.

15. Tests für C

Die C-Implementierung verwendet dieselben Testdateien.

Beispiel:

TEST("minimal.dat")
{
    myformat_document_t doc;

    CHECK(myformat_parse(data, size, &doc) == MYFORMAT_OK);
    CHECK(doc.version == 1);
    CHECK(doc.flags == 0);
}


Damit ist sichergestellt, dass C und Rust dieselben Dateien akzeptieren und gleich interpretieren.

16. Tests für Python

Python benötigt zwei Testebenen.

Binding-Test
def test_parse():
    data = Path("../testdata/valid/minimal.dat").read_bytes()

    document = myformat.parse(data)

    assert document.version == 1


Dieser Test prüft:

Python → PyO3 → Rust

API-Test

Später wird zusätzlich die Python-Abstraktion geprüft:

document = myformat.parse(data)

assert document.header.version == 1
assert len(document.entries) == 0

17. Tests für TypeScript

Beispiel:

const data = readFileSync("../testdata/valid/minimal.dat");

const document = parse(data);

assert.equal(document.version, 1);


Damit wird geprüft:

TypeScript → Node.js → N-API → napi-rs → Rust

18. Warum nicht einen gemeinsamen Testcode für alle Sprachen?

Die Testdaten sollten gemeinsam sein.

Der Testcode darf aber sprachspezifisch sein.

Empfohlene Struktur:

                    Testdaten
                       │
        ┌──────────────┼──────────────┐
        ▼              ▼              ▼
       Rust             C          Bindings
                                      │
                                 ┌────┴────┐
                                 ▼         ▼
                              Python      TS


Dadurch kann jede Sprache ihre eigene idiomatische Testumgebung verwenden.

Die gemeinsame Wahrheit sind:

.dat-Dateien
erwartete Ergebnisse
erwartete Fehler
Format-Spezifikation
19. Cross-Testing

Zusätzlich zu normalen Tests sollte es Cross-Testing geben.

Wenn Rust Dateien schreiben kann:

Rust serialize
      │
      ▼
   file.dat
      │
      ▼
 C parse


Und umgekehrt:

C serialize
      │
      ▼
   file.dat
      │
      ▼
Rust parse


Noch besser:

Rust
 │
 ▼
serialize
 │
 ▼
C
 │
 ▼
serialize
 │
 ▼
Rust


Am Ende wird das semantische Ergebnis verglichen.

Damit lassen sich Unterschiede zwischen den Implementierungen erkennen.

20. Fuzzing

Fuzzing ergänzt die deterministischen Testvektoren.

Testvektoren prüfen:

Dieses konkrete Verhalten muss exakt funktionieren.

Fuzzing prüft:

Beliebige Eingaben dürfen den Parser nicht zum Absturz bringen.

Zu testen sind insbesondere:

Integer Overflow
Integer Truncation
Out-of-bounds Reads
Out-of-bounds Writes
ungültige Offsets
ungültige Längen
übergroße Größenangaben
Memory