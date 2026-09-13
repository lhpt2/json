# CSON — Parser mit kommentarerhaltendem Round-Trip

## Projektziel

Rust-Implementierung von **CSON** (Cursive Script Object Notation, Kang Seonghoon),
einem strikten Superset von JSON. Anders als bestehende Implementierungen erhält
diese hier **Kommentare über einen Parse-/Schreib-Zyklus hinweg**, damit
Konfigurationsdateien programmatisch in ihren Werten geändert und mit
unveränderten Kommentaren zurückgeschrieben werden können.

Die Codebasis geht von einer leicht angepassten Kopie des `serde_json`-Parsers aus.

**Referenz:** <https://github.com/lifthrasiir/cson> (Spec, CC0-1.0) ·
<https://github.com/lifthrasiir/cson-rust> (alte Implementierung, `rustc-serialize`).
Der Spec-Text ist unter `docs/spec/` eingecheckt, inkl. Commit-Hash der Fassung,
gegen die implementiert wird. Der Autor behält sich Änderungen ohne Ankündigung
vor — deshalb wird nie gegen die Live-Fassung getestet.

**Nicht verwechseln mit** CoffeeScript Object Notation, die dieselbe Abkürzung und
dieselbe Dateiendung benutzt. Das gehört in den ersten Absatz des README.

Dateiendungen: `.cson` und `.csn` werden beide akzeptiert, `.csn` wird empfohlen.

---

## Formale Grammatik

CSON ist als Änderungsmenge gegen die ABNF von RFC 4627 (JSON) definiert.
`+` = Zusatz, `-` = ersetzte JSON-Zeile. Übrige JSON-Bedingungen (etwa
eindeutige Keys) gelten unverändert. Die Spec ist Public Domain.

```abnf
  JSON-text = object
            / array
+           / ws object-items

  begin-array     = ws %x5B ws    ; [ left square bracket
  begin-object    = ws %x7B ws    ; { left curly bracket
  end-array       = ws %x5D ws    ; ] right square bracket
  end-object      = ws %x7D ws    ; } right curly bracket
  name-separator  = ws %x3A ws    ; : colon
+                 / ws %x3D ws    ; = equal sign
  value-separator = ws %x2C ws    ; , comma
+                 / newline ws

  ws = *(
            %x20 /                ; Space
            %x09 /                ; Horizontal tab
-           %x0A /                ; Line feed or New line
-           %x0D                  ; Carriage return
+           newline-char /
+           comment
        )
+ newline = *(%x20 / %x09) newline-char
+ newline-char = %x0A             ; Line feed or New line
+              / %x0D             ; Carriage return
+ comment = sharp *comment-char
+ sharp = %x23                    ; # sharp
+ comment-char = %x00-09 / %x0B-0C / %x0E-10FFFF

  value = false / null / true / object / array / number / string

  false = %x66.61.6c.73.65        ; false
  null  = %x6e.75.6c.6c           ; null
  true  = %x74.72.75.65           ; true

- object = begin-object [ member *( value-separator member ) ] end-object
+ object = begin-object [ object-items ] end-object
+ object-items = member *( value-separator member ) [ value-separator ]
- member = string name-separator value
+ member = name name-separator value
+ name = string / bare-string

- array = begin-array [ value *( value-separator value ) ] end-array
+ array = begin-array [ array-items ] end-array
+ array-items = value *( value-separator value ) [ value-separator ]

  number = [ minus ] int [ frac ] [ exp ]
  decimal-point = %x2E            ; .
  digit1-9 = %x31-39              ; 1-9
  e = %x65 / %x45                 ; e E
  exp = e [ minus / plus ] 1*DIGIT
  frac = decimal-point 1*DIGIT
  int = zero / ( digit1-9 *DIGIT )
  minus = %x2D                    ; -
  plus = %x2B                     ; +
  zero = %x30                     ; 0

- string = quotation-mark *char quotation-mark
+ string = quotation-mark *dquoted-char quotation-mark
+        / apostrophe-mark *squoted-char apostrophe-mark
- char = unescaped /
-        escape (
+ dquoted-char = dquoted-unescaped / escaped
+ squoted-char = squoted-unescaped / escaped
+ escaped = escape (
+            %x27 /               ; '    apostrophe      U+0027
             %x22 /               ; "    quotation mark  U+0022
             %x5C /               ; \    reverse solidus U+005C
             %x2F /               ; /    solidus         U+002F
             %x62 /               ; b    backspace       U+0008
             %x66 /               ; f    form feed       U+000C
             %x6E /               ; n    line feed       U+000A
             %x72 /               ; r    carriage return U+000D
             %x74 /               ; t    tab             U+0009
             %x75 4HEXDIG )       ; uXXXX                U+XXXX
  escape = %x5C                   ; \
  quotation-mark = %x22           ; "
+ apostrophe-mark = %x27          ; '
- unescaped = %x20-21 / %x23-5B / %x5D-10FFFF
+ dquoted-unescaped = %x20-21 / %x23-5B / %x5D-10FFFF
+ squoted-unescaped = %x20-26 / %x28-5B / %x5D-10FFFF

+ verbatim-string = verbatim-fragment *(newline ws verbatim-fragment)
+ verbatim-fragment = pipe *verbatim-char
+ pipe = %x7C                     ; |
+ verbatim-char = %x20-10FFFF

+ bare-string = id-start *id-end

+ id-start = %x24 / %x2D / %x41-5A / %x5F / %x61-7A / %xAA / %xB5
+          / %xBA / %xC0-D6 / %xD8-F6 / %xF8-02FF / %x0370-037D
+          / %x037F-1FFF / %x200C-200D / %x2070-218F / %x2C00-2FEF
+          / %x3001-D7FF / %xF900-FDCF / %xFDF0-FFFD / %x10000-EFFFF
+ id-end = id-start / %x2E / %x30-39 / %xB7 / %x0300-036F / %x203F-2040
```

### Fallstricke in dieser Grammatik

Diese Punkte sind die Ursache der meisten Implementierungsfehler und müssen
jeweils als Fixture abgesichert werden.

**Kommentare sind Whitespace.** `comment` ist Teil von `ws`, also überall
erlaubt, wo Whitespace erlaubt ist — auch zwischen Key und Separator, zwischen
Separator und Wert, und zwischen zwei `verbatim-fragment`.

**`comment-char` schließt `%x0A` und `%x0D` aus.** Ein Kommentar endet also an
CR *oder* LF. `newline-char` ist ebenfalls beides einzeln; CRLF sind zwei
`newline-char`. Für die Zeilenanker-Regel bedeutet das: CR, LF und CRLF müssen
alle als *ein* Zeilenwechsel behandelt werden, sonst entsteht bei CRLF eine
Phantom-Leerzeile.

**Der Zeilenumbruch ist ein `value-separator`.** `value-separator = ws %x2C ws /
newline ws`. Ein weggelassenes Komma ist kein Sonderfall, sondern die zweite
Alternative der Regel.

**Verbatim-Strings:**
* `#` innerhalb eines Verbatim-Fragments ist **kein** Kommentar
  (`verbatim-char = %x20-10FFFF`). Die Zeilenanker-Regel darf hier nicht greifen.
* Ein Fragment endet am Zeilenumbruch, Escapes werden nicht verarbeitet.
* Aufeinanderfolgende Fragmente werden mit `\n` zu einem String verbunden.
* **Dokumentierte Ambiguität:** Im Array-Kontext lässt sich
  `verbatim-fragment newline ws verbatim-fragment` als ein einzelner
  `verbatim-string` *oder* als zwei Werte mit `value-separator` lesen. Die Spec
  schreibt die **erste** Lesart vor. Zwei getrennte Verbatim-Strings in einem
  Array brauchen ein explizites Komma oder eine Leerzeile dazwischen.
* **Offene Stelle in der Spec:** `verbatim-string` wird definiert, aber in
  `value` und `string` nicht referenziert. Das ist nach Einführungstext und
  Beispielen ein Versehen; wir behandeln Verbatim-Strings als zusätzliche
  Alternative von `value`. Entscheidung im Repo dokumentieren.

**`bare-string` nur als Key**, nie als Wert (`name = string / bare-string`).
`$` und `-` sind in jeder Position erlaubt, `$type` also unquotiert schreibbar.
Der Zeichenvorrat ist die Vereinigung aus ECMAScript-5-Identifier und
XML-1.0-Name, minus `:` — nicht die aktuelle Unicode-Definition, also fest
kodieren und nicht aus einer Unicode-Crate ableiten.

**Bare Root nur für Objekte** (`ws object-items`), nicht für Arrays.

**Trailing Comma** steckt in `[ value-separator ]` am Ende von `object-items`
und `array-items` — und ist damit auch ein weggelassenes Komma vor der
schließenden Klammer.

---

## Die zwei Achsen — nie vermischen

Das ist die wichtigste Unterscheidung im ganzen Projekt. Sie wurde in der
Designphase mehrfach durcheinandergebracht:

| | Woher | Gilt für |
|---|---|---|
| **Kommentare** | aus der Quelldatei, verbatim | Erhalt beim Round-Trip |
| **Layout** | aus `Style`, beim Parsen per First-Match erkannt | die **gesamte** Ausgabe |

Der Serializer schreibt **immer einheitlich** nach `Style`. Auch wenn die
Eingabedatei uneinheitlich formatiert war. Es gibt **kein** knotenweises
Verbatim-Durchreichen von Layout, **kein** `raw`-Slice für Stil-Zwecke, **keine**
Style-Felder pro Knoten.

Daraus folgt: `write(parse(s)) == s` gilt **nicht** und ist kein Bug.
Das Kriterium ist Idempotenz ab dem ersten Durchlauf (siehe Test-Invarianten).

---

## Architektur: drei Schichten

```
Schicht 1   Text  ->  Document          eigener Parser, kennt Trivia, kennt serde nicht
Schicht 2   impl serde::Deserializer for &Node        (analog serde_json::Value)
Schicht 3   impl serde::Serializer with Ok = Node
```

**Trivia lässt sich nicht durch serde transportieren.** Das `Deserializer`-Modell
kennt nur Werte; es gibt keinen Callback für "hier stand ein Kommentar". Das ist
eine Eigenschaft von serde, keine fehlende Funktion in serde_json.

Daraus ergeben sich zwei Wege durch die Bibliothek:

```
Wegwerf-Lesen:     text -> Document -> T
Config-Editing:    text -> Document -> [Mutation am Document] -> text
```

> **Der Editier-Pfad darf nie durch `T` laufen.** Wer nach `T` deserialisiert und
> `T` direkt serialisiert, bekommt einen Baum ohne Trivia. Das ist per Konstruktion
> so, nicht als Fehler. Für typisiertes Arbeiten gibt es `Document::merge_from`.

---

## Datenmodell

```rust
pub struct Document<'a> {
    root:  Node<'a>,
    /// Trivia nach dem letzten Token bis EOF
    suffix: Cow<'a, str>,
    /// Waren die äußeren Klammern in der Quelle weggelassen?
    bare_root: bool,
    style: Style,
}

pub struct Node<'a> {
    /// Whitespace + '#'-Kommentare vor diesem Knoten, roh.
    /// KEINE Satzzeichen: ',', ':' und '=' setzt der Serializer selbst.
    prefix: Cow<'a, str>,
    value:  Value<'a>,
}

pub enum Value<'a> {
    Null,
    Bool(bool),
    Number(Number<'a>),
    Str(CsonStr<'a>),
    Array  { items:   Vec<Node<'a>>,  trailing: Cow<'a, str> },
    Object { entries: Vec<Entry<'a>>, trailing: Cow<'a, str> },
}

pub struct Entry<'a> {
    key:   Node<'a>,   // eigener Node, damit Kommentare vor dem Key einen Slot haben
    value: Node<'a>,
}

pub struct Number<'a> {
    /// Rohliteral. Nicht wegen Formatierung, sondern wegen Wertverlust:
    /// u64/i64 jenseits 2^53 überleben den Umweg über f64 nicht.
    raw: Cow<'a, str>,
}
```

### Drei verschiedene Trivia-Slots — nicht verwechseln

* `Document::suffix` — nach dem letzten Token bis Dateiende
* `Array::trailing` / `Object::trailing` — vor der schließenden Klammer
* `Node::prefix` — vor dem Knoten

Es gibt **keinen** `Node`-Slot für Kommentare am Zeilenende. Die werden beim
Parsen umgehängt (siehe unten).

### Warum `Vec<Entry>` und keine Map

Duplicate Keys bleiben erhalten und können beim Validieren gemeldet werden, statt
still überschrieben zu werden. Lookup-Performance ist bei Konfigurationsgrößen
irrelevant.

---

## Trivia-Regeln

### Grundregel: Zeilenanker

Ein Kommentar, der **nicht** am Zeilenanfang steht, wird in den Trivia-Slot
**vor dem ersten Token seiner Zeile** verschoben. Anschließend existiert im Baum
kein Zeilenend-Kommentar mehr.

```
port = 8080  # nur intern          # nur intern
                          ->       port = 8080
```

### Ausnahme: Zeile beginnt mit schließender Klammer

Dann ist der Anker **die Zeile, in der die zugehörige öffnende Klammer steht**,
rekursiv nach derselben Regel aufgelöst. Ohne diese Ausnahme rutscht der
Kommentar ins Innere des Containers und beschreibt optisch das Falsche.

```
things: [                          # die Liste
  1,                               things: [
  2                          ->      1,
]  # die Liste                       2
                                   ]
```

### Implementierung: Lexer-Nachlauf, nicht im Parser

Die Regel ist rein textuell. Der Lexer produziert Tokens + Trivia, ein Pass
dazwischen hängt um, der Baumbau sieht danach nur noch Kommentare in Eigenzeilen.

```rust
let mut line_anchor: usize = 0;           // Slot vor dem ersten Token der Zeile
let mut at_line_start = true;
let mut stack: Vec<usize> = Vec::new();   // Anker der jeweiligen Öffnerzeile

for tok in &mut stream {
    if at_line_start {
        line_anchor = match tok {
            Close => *stack.last().unwrap_or(&0),   // Anker der Öffnerzeile erben
            _     => slot_before(tok),
        };
        at_line_start = false;
    }
    match tok {
        Open    => stack.push(line_anchor),   // den AUFGELÖSTEN Anker pushen
        Close   => { stack.pop(); }
        Newline => at_line_start = true,
        Comment(c) if !at_line_start => move_to(line_anchor, c),
        _ => {}
    }
}
```

Dass beim `Open` der bereits aufgelöste `line_anchor` gespeichert wird, erledigt
die Rekursion und den Fall `}, {` in einer Zeile.

**Beim Umhängen:** abschließenden Whitespace des Ziel-Slots wegschneiden, sonst
entsteht eine Zeile aus reinem Whitespace, die der Writer als Leerzeile ausgibt.
Mehrere Kommentare in denselben Slot: in Quellreihenfolge anhängen.

**Verbatim-Strings sind ausgenommen:** Ein `#` innerhalb eines Fragments ist
Teil des Strings, kein Kommentar. Der Umhäng-Pass darf dort nicht greifen — der
Lexer muss Verbatim-Fragmente also bereits als ein Token liefern.

**Bekannter Genauigkeitsverlust, akzeptiert:** Bei `b: 2, c: 3  # zu c` landet der
Kommentar vor `b`. Bei zeilenweise geschriebenen Konfigurationen irrelevant.

### Prefix-Inhalt

Nur Whitespace und `#`-Kommentare. Beim Setzen über die öffentliche API
validieren — ein Prefix mit anderem Inhalt erzeugt still ungültiges CSON.

---

## Serializer

### Einrückung neu erzeugen, Zeilenstruktur erhalten

Im gespeicherten Prefix steckt die Einrückung der *Quelldatei*. `Style` schreibt
eine eigene vor. Also: Kommentartext byteweise unverändert, Leerzeug davor neu.

```rust
fn write_prefix(out: &mut String, prefix: &str, depth: usize, style: &Style) {
    for line in prefix.lines() {
        let t = line.trim();
        out.push('\n');
        if !t.is_empty() {
            out.push_str(&style.indent(depth));
            out.push_str(t);              // '#...' unverändert
        }
    }
}
```

Leerzeilen zwischen Kommentarabsätzen bleiben dadurch erhalten — das ist der
Grund, warum Prefix ein roher String ist und keine strukturierte Kommentarliste.

### Satzzeichen

`,`, `:`/`=` und Klammern setzt ausschließlich der Serializer, nach `Style`.
Sie tauchen nie in Trivia auf.

---

## Style: First-Match-Erkennung

`Style` ist ein Struct aus `Option<T>`-Feldern. Jedes wird beim **ersten**
Auftreten in der Quelldatei einmal gesetzt (`get_or_insert_with`) und danach nicht
mehr verändert. `parse` liefert Dokument und Style zusammen.

Dimensionen bei vollem CSON:

* Separator `:` oder `=`
* Quote-Stil für Werte (`'` / `"`)
* Key-Stil (unquotiert / `'` / `"`)
* Kommas gesetzt oder bei Zeilenumbruch weggelassen
* Trailing Comma
* Einrückung (Zeichen + Breite)
* Top-Level-Klammern (`Document::bare_root`)
* `|`-Mehrzeilenstrings für mehrzeilige Werte

**Zwei Dimensionen haben keinen verlässlichen First-Match** und brauchen einen
expliziten Default statt Raten: Einrückung (erst am ersten verschachtelten
Element ablesbar) und `|`-Strings (in vielen Dateien gar nicht vorhanden).

Öffentlich: `doc.style()`, `doc.set_style(..)`. Ein Umstieg auf Mehrheitsentscheid
wäre später billig (dieselben Felder, Zähler statt Latch), ist aber nicht geplant.

---

## Editier-Operationen

### Löschen — Kommentare bleiben erhalten (Default)

```rust
// N löschen, Nachfolger S:
prefix(S) = prefix(N) + prefix(S)
// letztes Element: container.trailing = prefix(N) + container.trailing
```

Reines Aneinanderhängen. Keine Separator-Behandlung nötig, weil der Serializer
die Kommas selbst setzt. Optional Folgen von mehr als zwei `\n` auf zwei kappen.

Der Kommentar dokumentiert danach ein Feld, das es nicht mehr gibt, und steht
über dem nächsten. Das ist die bewusst gewählte Semantik.
Falls später gebraucht: `DeletePolicy { Keep, Drop }` am Aufruf.

### Einfügen

Neuer Knoten bekommt `prefix: ""`. Formatierung kommt ohnehin aus `Style`.

### Typisiert arbeiten: `merge_from`

```rust
let mut doc = cson::parse(&text)?;
let mut cfg: Config = doc.deserialize()?;
cfg.server.port = 9090;
doc.merge_from(&cfg)?;
fs::write(path, doc.to_string())?;
```

`merge_from` serialisiert `cfg` über Schicht 3 zu einem frischen Baum und gleicht
diesen gegen `doc` ab, statt es zu ersetzen:

* Objekte: gemeinsame Keys rekursiv; nur neu → anhängen; nur alt → entfernen
* Arrays: positionsweise, Rest anhängen/kürzen
* Skalare gleich → **nichts anfassen**
* Skalare ungleich → **nur `value` überschreiben, nie den ganzen Node**
  (sonst geht der Prefix mit)

**Der Gleichheitsvergleich muss Trivia ignorieren** — sonst ist nie etwas gleich.
Eigenes `PartialEq` bzw. `fn value_eq`.

**Zahlen numerisch vergleichen, nicht als Rohstring.** `1.50` aus der Datei und
`1.5` aus dem Struct sind derselbe Wert; Stringvergleich schreibt bei jedem
Speichern alle Zahlen neu. Rohliteral nur behalten, wenn der Wert gleich blieb.

**Achtung bei `#[serde(skip)]` und `skip_serializing_if`:** Zusammen mit der
Regel "nur alt → entfernen" löscht das still Einträge, die der Nutzer gesetzt
hatte. Diese Felder müssen von der Entfernen-Regel ausgenommen werden.

---

## Test-Invarianten

```rust
let a = write(&parse(s)?);
let b = write(&parse(&a)?);

// 1. Fixpunkt ab dem ersten Durchlauf (Identität gilt bewusst nicht)
assert_eq!(a, b);

// 2. Kommentare vollständig — als Menge, weil der erste Lauf umhängt
assert_eq!(comments(&parse(&a)?), comments(&parse(s)?));

// 3. Werte unverändert
assert_eq!(parse(&a)?.value(), parse(s)?.value());

// 4. Style-Fixpunkt: was der Writer ausgibt, muss denselben Style ergeben
assert_eq!(detect_style(&a), doc.style());
```

Invariante 4 fängt am meisten und wird leicht vergessen. Sie ist auch die
häufigste Ursache, wenn 1 fehlschlägt: irgendein Writer-Zweig ist inkonsistent
mit seiner eigenen Konfiguration.

Für `merge_from` zusätzlich: parsen, mit **unverändertem** Struct mergen,
schreiben — Ergebnis muss `a` entsprechen.

Für Löschen: jedes Element einzeln löschen, schreiben, neu parsen — muss gelingen,
Wert muss dem Original ohne dieses Element entsprechen, Kommentarmenge muss
identisch bleiben.

### Werkzeuge — erst die vorhandenen

**Zunächst ausschließlich mit der Testumgebung und den Tests arbeiten, die in
dieser Codebasis bereits vorhanden sind.** Die geerbte serde_json-Testsuite ist
der Ausgangspunkt: Sie deckt den JSON-Kern ab, den CSON als Superset vollständig
akzeptieren muss. Bestehende Tests anpassen statt daneben neue Infrastruktur
aufzubauen. Keine zusätzlichen Dev-Dependencies ohne Rückfrage.

Erst wenn Schicht 1 bis 3 stehen und die vier Invarianten auf den vorhandenen
Tests grün sind, wird über Ergänzungen entschieden. Kandidaten, in dieser
Reihenfolge, jeweils einzeln und nach Rücksprache:

* `insta` mit `glob!` für Fixture-Snapshots — Ausgabe ist deterministisch,
  Snapshots sind deshalb brauchbar
* `proptest` auf **Bäumen** (inkl. zufälliger Trivia-Strings), nicht auf Text —
  generierte Bäume sind per Konstruktion wohlgeformt
* `cargo-fuzz`: kein Panic; bei `Ok` muss der Round-Trip stabil sein
* `nst/JSONTestSuite`: alle `y_`-Dateien sind gültiges CSON und müssen
  akzeptiert werden. Abweichung bei `n_` nur dort, wo CSON bewusst erweitert.

### Fixtures

Kommentar-Platzierung:
* vor dem ersten Wert · nach dem letzten Wert am Dateiende
* in eigener Zeile vor `]`/`}` (kein Nachfolgeknoten → Container-Trailing)
* `[ /* leer mit Kommentar */ ]`, leeres Objekt mit Kommentar
* Datei nur aus Kommentaren; leere Datei
* zwischen Key und Separator; zwischen Separator und Wert; direkt nach Komma
* `#` am Dateiende ohne abschließenden Zeilenumbruch
* `#` innerhalb eines String-Literals (darf kein Kommentar sein)
* mehrere aufeinanderfolgende Kommentare mit Leerzeilen dazwischen

Zeilenanker-Regel gezielt:
* Kommentar hinter Skalar, hinter Closer, hinter Opener
* mehrere Closer in einer Zeile mit Kommentar: `}]  # x` → Anker ist die
  Öffnerzeile des **äußeren** Containers
* Kommentar an Öffner- **und** Schließerzeile → selber Slot, Quellreihenfolge

CSON-Syntaxvarianten:
* dieselben Daten in allen Schreibweisen; gemischt in einer Datei
  (`'a' = 1, "b": 2`)
* Dokument ohne Top-Level-Klammern (nur Objekt, nie Array)
* Komma weggelassen über einen Kommentar hinweg
* Trailing Comma, auch kombiniert mit Kommentar danach
* Bare Keys: `$type`, `-foo`, Nicht-ASCII; Bare-String als *Wert* muss
  abgelehnt werden

Verbatim-Strings:
* `#` innerhalb eines Fragments (kein Kommentar)
* Kommentar zwischen zwei Fragmenten
* zwei Fragmente in einem Array ohne Trennung → **ein** String
* dieselben zwei mit explizitem Komma bzw. Leerzeile → **zwei** Strings
* Fragment am Dateiende ohne abschließenden Zeilenumbruch
* Fragmente mit unterschiedlicher Einrückung

Sonstiges:
* CRLF durchgehend; CRLF/LF gemischt; BOM
* Integer jenseits 2^53; `1.50`; `1e3`; `-0`
* `"\u0041"` vs. `"A"`

---

## Reihenfolge beim Bauen

1. Schicht 1 (Lexer + Umhäng-Pass + Parser) mit Invarianten 1–3, ohne serde
2. Style-Erkennung + Writer, dann Invariante 4
3. Schicht 2 (`Deserializer for &Node`)
4. Schicht 3 (`Serializer with Ok = Node`) + `merge_from`

Nicht mit serde anfangen — dann entsteht ein Modell, in dem für Trivia
nachträglich kein Platz mehr ist.

## Was aus serde_json übernommen wird

Vor allem String-Unescaping (inkl. Surrogatpaare) und Zahlen-Parsing. Beides
liegt dort in privaten Modulen, wird also als Fork geführt. serde_json ist
MIT OR Apache-2.0; die Lizenztexte müssen mitgeführt werden. Herkunft in
`NOTICE` bzw. im Modulkopf dokumentieren.
