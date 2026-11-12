# Änderungsprotokoll

[简体中文](zh-CN.md) · [繁體中文](zh-TW.md) · [English](en-US.md) · [Español](es-ES.md) · **Deutsch** · [日本語](ja-JP.md) · [Français](fr-FR.md) · [Русский](ru-RU.md)

Für Benutzer sichtbare Änderungen der einzelnen Argus-Versionen. Die Versionsnummern folgen der [semantischen Versionierung](https://semver.org/lang/de/). Diese Datei ist eine Übersetzung des [vereinfachten chinesischen Änderungsprotokolls](zh-CN.md) und entspricht ihm Eintrag für Eintrag.

## [0.5.0](https://github.com/Ray-lea/Argus/compare/v0.4.0...v0.5.0) - 2026-11-12

v0.5.0 ergänzt das Anhalten und Fortsetzen von Prozessen, die Gruppierung von Prozessen nach Benutzer, vier Spalten mit Prozessinformationen und nach dem Inhalt berechnete Spaltenbreiten. Die Geräteliste der Leistungsseite unterstützt das Ausblenden und Sortieren von Geräten. Die Oberfläche ist jetzt auch auf traditionellem Chinesisch, Spanisch, Deutsch, Japanisch, Französisch und Russisch verfügbar. Diese Version verhindert außerdem, dass einige Systeme mit Intel RST nach dem Start einfrieren. Unter anderem ist der Fehler behoben, dass sich USB-Datenträger nicht sicher entfernen ließen. Bei ausgeblendetem Fenster sinkt der Speicher- und CPU-Verbrauch.

### Neue Funktionen

- **Prozesse anhalten und fortsetzen**: Das Kontextmenü von Prozessen und App-Gruppen enthält die neuen Befehle „Prozess anhalten“ und „Prozess fortsetzen“. Vor dem Anhalten erläutert Argus die Auswirkungen und fordert eine Bestätigung an. Kritische Systemprozesse und Argus selbst lassen sich nicht anhalten. Hinter dem Namen eines angehaltenen Prozesses erscheint die Markierung „Angehalten“. ([@digital-bombyx](https://github.com/digital-bombyx) [#65](https://github.com/Ray-lea/Argus/pull/65))
- **Spalten mit Prozessinformationen**: Vier neue optionale Spalten sind verfügbar: „Dateiversion“, „Architektur“, „Erhöht“ und „Befehlszeile“. Standardmäßig sind sie ausgeblendet. Aktivieren lassen sie sich im Kontextmenü der Spaltenköpfe oder unter „Spalten auswählen“. ([@digital-bombyx](https://github.com/digital-bombyx) [#72](https://github.com/Ray-lea/Argus/pull/72))
- **Breite der Prozessspalten**: Automatische Spaltenbreiten richten sich nach dem Inhalt. Der verbleibende Platz bleibt rechts in der Tabelle frei. Durch Ziehen einer Spaltengrenze lässt sich die Breite einer einzelnen Spalte anpassen. Ein Doppelklick auf die Spaltengrenze stellt die automatische Breite wieder her. Das Menü der Spaltenköpfe enthält die neue Option „Volle Breite“. Ist sie aktiviert, nimmt die Spalte „Name“ die verbleibende Breite ein.
- **Prozesse nach Benutzer gruppieren**: Unter „Einstellungen → Sprache und Aussehen → Prozesse nach Benutzer gruppieren“ stehen „Automatisch“, „Immer gruppieren“ und „Nie gruppieren“ zur Wahl. Bei „Automatisch“ gruppiert Argus nur, wenn mehrere Benutzer angemeldet sind. Das Kontextmenü einer Gruppenzeile bietet „Benutzerkonten verwalten“ und „Abmelden“.
- **Systemprozesse ausblenden**: Die Seiten „Prozesse“ und „Verbindungen“ haben einen neuen Schalter „Systemprozesse ausblenden“. Er ist standardmäßig ausgeschaltet. Jede Seite speichert ihre Einstellung separat. Die Listen der Programme, die immer angezeigt oder zusätzlich ausgeblendet werden, lassen sich auf der Seite „Einstellungen“ bearbeiten. ([#32](https://github.com/Ray-lea/Argus/issues/32))
- **Prozessbaum und Ressourcenübersicht**: Der Prozessbaum ist standardmäßig reduziert. Das Kontextmenü der Spaltenköpfe bietet „Alle erweitern“ und „Alle reduzieren“. Die „Ressourcenübersicht“ lässt sich auf „Automatisch“, „Immer anzeigen“ oder „Ausgeblendet“ einstellen. Bei „Automatisch“ wird die Übersicht eingeklappt, wenn die Fensterhöhe nicht ausreicht.
- **Geräteliste der Leistungsseite**: Per Rechtsklick lässt sich ein Gerät ausblenden. Im Untermenü „Angezeigte Geräte“ können Geräte auch einzeln ausgewählt werden. Die Reihenfolge der Geräte ändert sich durch Ziehen. Datenträger können nach Datenträgernummer, nach Laufwerksbuchstabe oder in benutzerdefinierter Reihenfolge angeordnet werden.
- **CPU-Mehrkernansicht**: Die kleinen Diagramme der logischen Prozessoren zeigen die Kernelzeit. Diese Anzeige lässt sich im Kontextmenü des CPU-Diagramms ausschalten. Argus erkennt stromsparende Effizienzkerne von Intel (LP-E-Kerne) und stellt sie in einer eigenen Farbe dar. Diese Kerne lassen sich gesondert filtern. Die kleinen Diagramme werden bevorzugt in Zeilen und Spalten ohne Lücken angeordnet.
- **Hardwareinformationen**: Die GPU-Seite zeigt Treiberversion, Treiberdatum und DirectX-Featureebene. Die Datenträgerdetails zeigen für weitere SATA-SSDs die verbleibende Lebensdauer und die insgesamt gelesenen und geschriebenen Datenmengen. Für Festplatten (HDD) sind Betriebs- und Sektorstatistiken neu. Die SMART-Attributtabelle passt ihre Breite an den Inhalt an.
- **Anzeigesprachen**: Neu sind traditionelles Chinesisch, Spanisch, Deutsch, Japanisch, Französisch und Russisch. Oberfläche, Infobereich, Systemmeldungen und Installationsprogramm sind übersetzt. Zahlen und Datumsangaben verwenden das regionale Format von Windows. ([@abc0922001](https://github.com/abc0922001) [#45](https://github.com/Ray-lea/Argus/pull/45), [@Rimuwu](https://github.com/Rimuwu) [#51](https://github.com/Ray-lea/Argus/pull/51), [@s0ftw4r8](https://github.com/s0ftw4r8) [#52](https://github.com/Ray-lea/Argus/pull/52), [@khativari](https://github.com/khativari) [#55](https://github.com/Ray-lea/Argus/pull/55), [@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59) [#62](https://github.com/Ray-lea/Argus/pull/62))
- **Neuerungen**: Die Seite „Info“ und der Update-Dialog zeigen die Neuerungen in der Anzeigesprache. Unter „Einstellungen → Updates“ öffnet jetzt die Schaltfläche „Neuerungen anzeigen“ den Update-Dialog. Der Dialog bietet „Alle erweitern“ und „Alle reduzieren“.
- **Symbole und Animationen**: Die Symbole der Oberfläche stammen jetzt aus der Symbolbibliothek Lucide. Bewegt sich der Zeiger auf eine Schaltfläche oder ein Navigationselement, spielt das Symbol einmal eine kurze Animation ab. Ist „Oberflächenanimationen“ ausgeschaltet, entfällt diese Animation.
- **Fenster und Infobereich**: Neu ist ein eigener Schalter „Fensterübergänge“. „Aus dem Infobereich öffnen“ lässt sich auf „Standardseite öffnen“ einstellen. Die Standardgröße des Fensters beträgt jetzt 1000×680. Die Mindestgröße sinkt auf 640×480. ([#27](https://github.com/Ray-lea/Argus/issues/27), [#40](https://github.com/Ray-lea/Argus/issues/40))
- **Task-Manager-Ersetzung**: „Task-Manager ersetzen“ bietet die neue Schaltfläche „Windows-Task-Manager einmalig öffnen“. Diese Schaltfläche öffnet den integrierten Task-Manager von Windows. Die Ersetzungseinstellungen bleiben unverändert.
- **Hinweise und Erinnerungen**: Die portable Version erläutert in der Einführung und in der rechten unteren Ecke die Unterschiede zur installierten Version. Außerdem bietet sie Links zum Herunterladen der installierten Version. Neu ist die „Stern-Erinnerung“. Die Erinnerung erscheint höchstens einmal pro Tag. Unter „Einstellungen → Updates“ lässt sie sich ausschalten. Die „Hinweise“ über „Dieser PC“ lassen sich einklappen.

### Leistung

- **Speicherverbrauch im Hintergrund**: Neu ist die Einstellung „Speicher bei ausgeblendetem Fenster freigeben“. Ist das Fenster für die eingestellte Dauer (standardmäßig 10 Minuten) minimiert oder im Infobereich verborgen, gibt Argus die Diagrammbilder frei. Ungenutzten Speicher gibt Argus dann an Windows zurück. Sind alle Fenster ausgeblendet, hält Argus den Anzeigetakt an. ([@SpookySandwich](https://github.com/SpookySandwich) [#54](https://github.com/Ray-lea/Argus/pull/54))

### Fehlerbehebungen

- **Einfrieren des Systems**: Argus fragt die Temperatur von Datenträgern hinter RAID-Controllern (etwa Intel RST) nicht mehr ab. Einige Treiberversionen reagierten nicht auf diese Abfrage. Das System fror dann wenige Sekunden nach dem Start von Argus ein. Die Temperatur dieser Datenträger wird als unbekannt angezeigt. ([#48](https://github.com/Ray-lea/Argus/issues/48))
- **Sicheres Entfernen von USB-Datenträgern**: Die Datenträgerüberwachung belegt das Gerät nicht mehr dauerhaft. USB-Datenträger lassen sich jetzt sicher entfernen. ([#57](https://github.com/Ray-lea/Argus/issues/57))
- **Datenträgerliste und Leistungsdaten**: Einige NVMe-Datenträger zeigten nur Geräteinformationen ohne Leistungsdaten. Außerdem verschwanden Datenträger aus der Liste, wenn keine Leistungszähler verfügbar waren. Beide Fehler sind behoben.
- **Prozessdaten**: Bei langsamen Systemabfragen werden Prozesswerte weiter aktualisiert und fehlen nicht mehr in großem Umfang. Beendete Prozesse bleiben nicht mehr in der Liste. Bei angehaltener Aktualisierung zeigt ein Prozess nach einer Aktion jetzt den neuen Zustand. ([#66](https://github.com/Ray-lea/Argus/issues/66))
- **Seite „Verbindungen“**: Verbindungen ohne zugehörigen Prozess erscheinen nicht mehr als „System Idle Process“.
- **CPU-Messwerte**: Prozessoren mit Hybridarchitektur zeigen jetzt die korrekte Geschwindigkeit. Einige Ryzen-Prozessoren mit sechs Kernen zeigen jetzt korrekte Spannungs- und Temperaturwerte je Kern. Messwerte interner Intel-Sensoren setzen nicht mehr aus, und die Temperaturkurve wird nicht mehr unterbrochen. ([#58](https://github.com/Ray-lea/Argus/issues/58))
- **Freigabe und Deinstallation**: Nach einer Änderung der Berechtigungen von Program Files oder des Laufzeitordners von Argus sind der Widerruf der Freigabe und die Deinstallation jetzt möglich. Kernelsensoren können einen bereits laufenden PawnIO-Treiber verwenden. Nach der Deinstallation bleiben der Ordner „Argus Sensor Setup“ und ein leerer Ordner „Argus Runtime“ nicht mehr zurück. ([#46](https://github.com/Ray-lea/Argus/issues/46))
- **Startberechtigungen**: Nach der Installation oder einem Update in der Anwendung startet Argus mit den Standardrechten des am Desktop angemeldeten Kontos. Dies entspricht dem Start über eine Verknüpfung. Bei Ausführung als Administrator kann Argus einen Windows-Explorer mit erhöhten Rechten neu starten.
- **Fensterdarstellung**: Nach dem Wiederherstellen der Fenstergröße erscheint die Oberfläche nicht mehr nur in der oberen linken Ecke. Nach einem Kaltstart über die Task-Manager-Ersetzung wird ein maximiertes Fenster vollständig angezeigt. Unter Windows 11 21H2 ist der Fensterhintergrund nicht mehr transparent. Nach dem Wechsel zu einer anderen App bleibt das Fenster nicht mehr vor anderen Fenstern. ([#44](https://github.com/Ray-lea/Argus/issues/44), [#37](https://github.com/Ray-lea/Argus/issues/37), [#35](https://github.com/Ray-lea/Argus/issues/35))
- **Infobereich**: Wird das Fenster beim Schließen im Infobereich verborgen, bleibt die CPU-Last nicht mehr erhöht. Nach einem Update in der Anwendung wandert das Infobereichssymbol nicht mehr in den Überlaufbereich. ([#31](https://github.com/Ray-lea/Argus/issues/31))
- **Oberflächendarstellung**: Dynamische Zahlen springen beim Aktualisieren nicht mehr horizontal. Der Update-Dialog bleibt beim Laden zentriert. Der Dialog „Welche Prozesse verwenden die Datei?“ bewegt sich nicht mehr ruckartig, wenn die Ergebnisse erscheinen. Kontextmenüs von Textfeldern erscheinen nicht mehr immer auf Englisch. ([#39](https://github.com/Ray-lea/Argus/issues/39))
- **Stabilität**: Argus beendet sich nicht mehr bei fehlerhaftem Anzeige-Timing oder bei wiederholt fehlgeschlagener Aufzählung der Netzwerkadapteradressen. Die Oberfläche friert nicht mehr ein, wenn die Wiederherstellung des Grafikgeräts und das Schließen eines Fensters gleichzeitig erfolgen.
- **Große Schriftgröße, Prozessregeln und Autostart-Einträge**: Bei großer Schriftgröße überlappen sich Messwerte und Spezifikationen auf der Leistungsseite nicht mehr. Enthält ein Programmname Nicht-ASCII-Buchstaben, fügt das Prozessmenü keine doppelte Regel mehr hinzu. Argus stürzt beim Lesen der Startaufgaben von App-Paketen nicht mehr ab. ([@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59))

### Hinweise zur Aktualisierung

- **Privilegierter Dienst**: Diese Version hebt das Kommunikationsprotokoll des Dienstes auf Version 11 an. Ein Update der installierten Version über das Installationsprogramm oder in der Anwendung aktualisiert zugleich den installierten Dienst und die Task-Manager-Ersatzkopie. Bei der portablen Version klicken Sie nach dem Austausch der Programmdatei unter „Einstellungen → Berechtigungen → Privilegierter Dienst“ auf „Dienst aktualisieren“ und bestätigen Sie die Administratoranfrage.
- **Einstellungen und Verlauf**: Das Update behält vorhandene Einstellungen und den Verlauf bei. Beim ersten Start übernimmt „Fensterübergänge“ die Einstellung für Fensterübergänge aus der bisherigen Option „Oberflächenanimationen“.

## [0.4.0](https://github.com/Ray-lea/Argus/compare/v0.3.4...v0.4.0) - 2026-11-06

v0.4.0 stellt die Oberfläche vollständig von Tauri, SolidJS und WebView2 auf die native Architektur Rust + GPUI Kit um. Der Ressourcenbedarf der Browser-Engine entfällt; natives DirectX-Rendering und Interaktionen wurden neu aufgebaut. Animationen bei hohen Bildwiederholraten, Echtzeitdiagramme, Abtastungsaktualisierungen der Listen und der Ressourcenverbrauch im Hintergrund wurden umfassend optimiert. Hinzu kommen anpassbare Prozessspalten, Datenträgerzustand und SMART-Details, weitere Messwerte integrierter Intel-Grafik sowie neue Fensterlayouts. Die [Argus-Website](https://argustask.com/de/) ist jetzt verfügbar und an den offiziellen Update-Spiegelserver angebunden.

### Neue Funktionen

- **Anpassbare Prozessspalten**: 40 Spalten in sechs Kategorien: allgemeine Informationen, CPU, Arbeitsspeicher, Datenträger und I/O, Netzwerk sowie GPU. Neu sind dedizierter und gemeinsamer GPU-Speicher, GPU-Engine, Abbildname, virtueller Adressraum, maximaler Arbeitssatz, Seitenfehler und kumulierte I/O-Zähler. Virtueller Adressraum und maximaler Arbeitssatz werden je Prozess angezeigt. Das Kontextmenü der Spaltenköpfe bietet Kategorie-Untermenüs und erlaubt das fortlaufende Ankreuzen; „Spalten auswählen“ unterstützt Kategoriefilter, Suche und Sortierung in zwei Bereichen. Beim Ziehen von Spaltenköpfen oder ausgewählten Spalten erfolgt die Umordnung sofort mit fließendem Ausweichen. Automatische Breiten reservieren Platz entsprechend dem Zahlenformat und verhindern Sprünge; manuelle Breiten sind ebenfalls möglich. „Name“ in der flachen Liste und im Prozessbaum entspricht der App-Ansicht: Vorrang haben App-Name oder Dateibeschreibung, der Name der ausführbaren Datei erscheint als sekundärer Text oder unter „Abbildname“.
- **Datenträgerzustand und Details**: Die Leistungsseite für Datenträger bietet „Übersicht / Details“. Informationen von NVMe-, ATA/SATA-, USB-SAT- und SCSI/SAS-Geräten werden bei Bedarf einmalig gelesen. Angezeigt werden Zustand, verbleibende Lebensdauer, Temperatur und Schwellenwerte, Betriebsstunden und Einschaltvorgänge, gelesene und geschriebene Datenmengen, Medien- und Lese-/Schreibfehler, PCIe-Verbindung und SMART-Attribute. Erneutes manuelles Einlesen ist möglich. Seriennummern stammen bevorzugt aus der vom Protokoll gelieferten Herstellerkennung und sind standardmäßig ausgeblendet.
- **Systemübersicht und Leistungsdiagramme**: Die Übersichtsleiste oben auf der Seite Prozesse wurde neu gestaltet. Kurven füllen die Karten bis zum Rand und verblassen unter den Zahlen links; oben steht die Skalenobergrenze, beim Überfahren wird die gesamte Karte hervorgehoben. Neu sind CPU-Temperatur sowie CPU- und GPU-Leistungsaufnahme. Die CPU-Übersicht auf der Leistungsseite fasst Paket- und Kernleistungsaufnahme unter „Leistung“ zusammen (bevorzugt Paketleistung, Quelle beim Überfahren; beide Kurven bleiben unter Details erhalten). Datenträger- und Netzwerkdiagramme skalieren in festen Stufen. Sekundäre Datenreihen kompakter Diagramme, etwa der Gerätevorschau in der Seitenleiste, verwenden halbtransparente durchgezogene Linien für bessere Lesbarkeit.
- **Messwerte integrierter Intel-Grafik**: Erweiterte Temperatur-, Takt-, Spannungs- und Leistungswerte des Treibers. Kernelsensoren (Modul IntelMCHBAR) und RAPL PP1 ergänzen bei integrierter Grafik der 10. Core-Generation (Comet Lake S/H) Temperatur, Takt, gemeinsamen Speichertakt, Kern-VID, GT-/Medienaktivität und Leistungsaufnahme. Abgeschaltete Takte von Grafikslices werden gesondert erkannt, Werte unter 1 W angezeigt und die Leistungsaufnahme der integrierten Grafik der zugehörigen GPU-Seite zugeordnet. Gültige RAPL-Werte haben Vorrang vor Nullwerten der Windows-Leistungszähler; nicht von Hardware oder Treiber gelieferte Angaben bleiben unbekannt.
- **Fenster und Oberflächenlayout**: Unter „Einstellungen → Autostart und Infobereich“ lässt sich die Startseite wählen. Die ausgeklappte Seitennavigation kann am rechten Rand verbreitert werden (Doppelklick stellt die Standardbreite wieder her). Der Detailbereich bietet „Wird angezeigt, wenn ein Prozess ausgewählt ist“: Er gleitet bei Auswahl heraus und wird ohne Auswahl automatisch eingeklappt. Beim Wechsel des Zeitbereichs zeigt Verlauf stets den neuesten Zeitraum bis zur Gegenwart. Die Mindestgröße beträgt 1240×800 für ein vollständiges Layout; ist der Arbeitsbereich des Hauptbildschirms kleiner, passt sich das Fenster automatisch an.
- **Info-Seite, Änderungen und offizieller Spiegelserver**: „Info“ ist eine eigene Navigationsseite (Strg+8) mit Linkkarten für Argus-Website und GitHub-Repository. Links in der Statusleiste öffnet ein Versionsabzeichen per Klick die Änderungen der aktuellen Version; per Rechtsklick lässt sich die Versionsnummer kopieren. Änderungen sind nach Kategorien gruppiert, neue Funktionen erscheinen in zweispaltigen Karten. Bei fehlgeschlagener GitHub-Verbindung versuchen automatische Updates zuerst den offiziellen Argus-Spiegelserver und danach GitHub-Proxys. Installationspakete und Änderungshinweise werden gleichermaßen per Signatur geprüft.

### Leistung

- **Reaktionsfähigkeit und Animationen bei hohen Bildwiederholraten**: Weniger Aufwand im Hauptthread für Abtastungsaktualisierungen, Textmessung und wiederholtes Zeichnen. Statische Diagrammhintergründe und Kompositionsebenen werden wiederverwendet, große Kurventexturen parallel auf mehreren Threads gezeichnet. Bei ausgeschalteter hardwarebeschleunigter GPU-Planung (HAGS) sinkt die GPU-Last der Diagramme deutlich. Seitenwechsel, Detailbereich, Listenscrollen und Spaltenumordnung laufen flüssig mit der Bildwiederholrate des Displays; Mehrkernraster und GPU-Engine-Diagramme passen ihre Größe im selben Frame wie das Fenster an.
- **Seitenwechsel und Hintergrundbetrieb**: Nach dem Start werden Dienst-, Autostart- und Verbindungslisten im Hintergrund vorgeladen und beim ersten Öffnen sofort angezeigt. Oberflächenpräferenzen werden gebündelt und asynchron im Hintergrund gespeichert, wodurch Pausen beim Seitenwechsel entfallen. Seiten ohne Echtzeitdiagramme überspringen wiederholte Neuaufbauten und senken die CPU-Last. Bei minimiertem oder im Infobereich verborgenem Fenster stoppt das Zeichnen; Diagrammebenen-Caches, veraltete Symbole und Texturatlanten werden bei Bedarf freigegeben, um Arbeits- und Grafikspeicher im Hintergrund zu sparen.
- **Hohe Auslastung und Speichermangel**: Ein eigener Thread bündelt Aktualisierungen des Infobereichs und verbessert Bedienung und Abtastungsreaktion bei hoher Last oder blockierter Shell. Bei Speichermangel werden Diagrammpuffer und Texturpräzision automatisch reduziert, damit Echtzeitkurven erhalten bleiben; bei ausreichendem Speicher kehrt die volle Präzision zurück. Die Fensteraktivierung kommt besser mit großflächig verdeckenden, stets im Vordergrund liegenden Fenstern zurecht.

### Fehlerbehebungen

- **Netzwerkverkehr je Prozess**: Dauerhafte Nullwerte auf manchen Rechnern sowie abwechselnde Nullwerte oder verzögerte Aktualisierungen bei einem Intervall von 0,5 s wurden behoben.
- **Privilegierter Dienst**: Unerwartetes Dienstende bei einer vorzeitigen Client-Trennung vor Abschluss der Pipe-Verbindung sowie Zeitüberschreitungen und fälschlich fehlende Daten bei häufigem Lesen wurden behoben.
- **App-Namen und Symbole**: Paketierte Apps (MSIX) zeigen jetzt den App-Namen des System-Task-Managers statt der Beschreibung der ausführbaren Datei. Fehlende Symbole und Dateibeschreibungen bei fehlenden Gerätepfadzuordnungen oder verweigertem Prozesszugriff wurden ebenfalls behoben.
- **GPU-Speicher je Prozess**: Residente Speicherzähler werden je Prozess und physischem Adapter dedupliziert. Dies korrigiert überhöhte oder mehrfach summierte Werte auf manchen Systemen und Treibern und unterscheidet fehlende Messwerte von null Verbrauch.
- **Leistungsanzeige integrierter Intel-Grafik**: Das wiederholte Erscheinen und Verschwinden der Leistungsaufnahme ruhender integrierter Grafik in der CPU-Übersicht wurde behoben.
- **CPU-Virtualisierungsstatus**: Hyper-V-Hosts mit aktivierter Hardwarevirtualisierung zeigen nicht mehr fälschlich „Deaktiviert“.
- **Freigabe des Administratorstarts**: Fehlgeschlagene Freigaben durch SID-Auflösung bei identischem Benutzer- und Computernamen wurden behoben. Neu erstellte Kopien derselben Version werden besser erkannt.
- **Speicherverbrauch bei langer Laufzeit**: Programmsymbole werden nach Inhalt dedupliziert, Metadaten-Caches beendeter Programme regelmäßig bereinigt und wartende Anfragen bei ausgelastetem Parser begrenzt. Dadurch wächst der Speicher nicht mehr ständig an, wenn häufig Programme von neuen Pfaden gestartet werden.
- **Externe Links**: Links zu Website, Repository und Release-Seiten werden direkt an die System-Shell übergeben. Die Verzögerung durch einen zwischengeschalteten neuen Explorer-Prozess entfällt.

### Hinweise zur Aktualisierung

- **Systemanforderungen**: Windows 10 22H2 (Build 19045) oder neuer einschließlich Windows 11, nur x64; Grafikgerät und Treiber mit DirectX-11-Unterstützung. Die Oberfläche benötigt die WebView2-Laufzeit nicht mehr.
- **Einstellungen und Verlauf**: Anwendungseinstellungen und Verlaufsdateien bleiben erhalten. Beim Update von v0.3.3 oder v0.3.4 werden separat gespeicherte Oberflächenpräferenzen übernommen. Beim direkten Update von v0.3.2 oder älter werden ausschließlich im alten WebView2-Cache gespeicherte Präferenzen (etwa Layout, Zeilennotizen und angeheftete Einträge) auf Standardwerte zurückgesetzt und müssen neu eingestellt werden. Alte WebView2-Daten von Argus werden automatisch im Hintergrund bereinigt.
- **Privilegierter Dienst**: Das Dienstprotokoll wird auf Version 10 angehoben. Ein Update der installierten Version über Installationsprogramm oder Anwendung aktualisiert zugleich den installierten Dienst und die Task-Manager-Ersatzkopie. Nach Austausch der portablen Programmdatei ist unter „Einstellungen → Berechtigungen → Privilegierter Dienst“ die Aktion „Dienst aktualisieren“ mit Administratorbestätigung erforderlich.

## [0.3.4](https://github.com/Ray-lea/Argus/compare/v0.3.3...v0.3.4) - 2026-10-26

### Fehlerbehebungen

- **Explorer-Neustart**: Die fehlende Wiederherstellung von Desktop und Taskleiste wurde behoben, wenn der alte Prozess während der Bereinigung beim Beenden oder der Fehlerberichterstattung hängen bleibt. Nach einer Zeitüberschreitung beim normalen Beenden wird nur die bestätigte alte Instanz beendet; erst nach ihrem bestätigten Ende wird der Desktop wiederhergestellt. Hat Windows den Desktop bereits wiederhergestellt, wird die vorhandene Instanz verwendet, um doppelte Starts zu vermeiden. ([#20](https://github.com/Ray-lea/Argus/issues/20))

### Hinweise zur Aktualisierung

- Beim Update von v0.3.x bleiben vorhandene Einstellungen und Verlaufsdaten erhalten.

## [0.3.3](https://github.com/Ray-lea/Argus/compare/v0.3.2...v0.3.3) - 2026-10-26

Diese Version ergänzt Oberflächenskalierung und Schriftgröße, verbessert das Speichern von Fenster- und Listenpräferenzen und passt die GPU-Auslastungsstatistik an.

### Neue Funktionen

- **Oberflächenskalierung**: Unter „Einstellungen → Sprache und Aussehen“ ist eine Skalierung von 80 % bis 150 % möglich, standardmäßig 100 %. Ganze Prozentwerte wie 113 % können eingegeben werden.
- **Schriftgröße**: Die Basisschriftgröße ist von 11 bis 20 px einstellbar, standardmäßig 13 px. Sie lässt sich mit der Skalierung kombinieren, wirkt sofort und wird automatisch gespeichert. Beide Einstellungen können unabhängig zurückgesetzt werden.

### Fehlerbehebungen

- **GPU-Statistik**: Versuch einer Korrektur zu niedriger Gesamtauslastung bei bestimmten Rechenlasten, ungewöhnlicher Spitzen beim Fortsetzen der Abtastung und überhöhter Summen mehrerer GPUs oder Engines. Prozessbezogene Engine-Zähler und verfügbare NVIDIA-Kernauslastung ergänzen die Werte; die Statistikquelle wird angezeigt. ([#19](https://github.com/Ray-lea/Argus/issues/19))
- **Fensterwiederherstellung**: Nach Beenden und Neustart werden Größe, Position und maximierter Zustand wiederhergestellt. Ein minimiert beendetes Fenster bleibt beim nächsten Start nicht minimiert; außerhalb des Bildschirms liegende Fenster werden in den verfügbaren Anzeigebereich zurückgeholt.
- **Listenpräferenzen**: Prozess-, Dienst-, Autostart- und Verbindungslisten speichern Sortierspalte und Richtung getrennt. Dienste und Verbindungen stellen auch die gewählten Filter wieder her.
- **Speichern von Präferenzen**: Oberflächenpräferenzen werden getrennt gespeichert und aus älteren Caches migriert. Gespeicherte Präferenzen bleiben nach dem Leeren des Oberflächen-Caches verfügbar.
- **Layout und Fensterschaltflächen**: Symbolleiste und Navigation passen sich bei großer Schrift oder hoher Skalierung an schmale Fenster an; der Trefferbereich der Maximieren-Schaltfläche wurde korrigiert.

### Hinweise zur Aktualisierung

- Beim Update von v0.3.x bleiben Einstellungen und Verlauf erhalten. Alte Oberflächenpräferenzen werden beim ersten Start automatisch migriert; manuelles Neueinstellen entfällt.

## [0.3.2](https://github.com/Ray-lea/Argus/compare/v0.3.1...v0.3.2) - 2026-10-23

Diese Version ersetzt die zurückgezogene v0.3.1. Sie behebt, dass die installierte Version nach einem Administratorstart oder Task-Manager-Aufruf fälschlich als portable Version erkannt wurde und dadurch keine Updates in der Anwendung möglich waren.

### Hinweise zur Aktualisierung

- Zeigt eine installierte Kopie „Die portable Version wird nicht automatisch aktualisiert“, kann das Installationsprogramm dieser Release-Seite heruntergeladen und direkt ausgeführt werden. Eine vorherige Deinstallation von Argus ist nicht erforderlich. Nach dem Update funktionieren integrierte Updates auch beim Administratorstart und über Task-Manager-Aufrufe.

### Fehlerbehebungen

- **Updates der installierten Version**: Administrator- und Task-Manager-Ersatzkopien werden korrekt erkannt. Automatische Suche, Download und Installation von Updates funktionieren wieder.
- **Erkennung portabler Versionen**: Ein Deinstallationsprogramm neben der Programmdatei führt nicht mehr zur Einstufung als installierte Version. Updates werden weiterhin manuell von der Release-Seite geladen.

### Enthaltene Änderungen

- Enthält den Administratorstart nach einmaliger Freigabe aus v0.3.1 sowie Korrekturen für Explorer-Neustart, CPU-Statistik, Diagramme nach Fensterwiederherstellung, Textcursor in der Prozessliste und Updateinstallation.

## [0.3.1](https://github.com/Ray-lea/Argus/compare/v0.3.0...v0.3.1) - 2026-10-23

**Diese Version wurde zurückgezogen. Ihre Funktionen und Fehlerbehebungen sind in v0.3.2 enthalten.**

Neu ist der Administratorstart nach einmaliger Freigabe mit einheitlichen Rechten bei Anmeldung, Verknüpfungen und Task-Manager-Aufrufen. Probleme beim Explorer-Neustart, bei der CPU-Auslastung und bei der Updateinstallation wurden behoben.

### Hinweise zur Aktualisierung

- **Als Administrator ausführen**: Standardmäßig ausgeschaltet; aktivierbar unter „Einstellungen → Berechtigungen“ oder in der Einführung. Erfordert ein Administratorkonto. Argus startet nach dem Aktivieren neu und behält Einstellungen und Verlauf. Privilegierter Dienst, Kernelsensoren und Task-Manager-Ersetzung werden weiterhin einzeln freigegeben.
- **Installierte Version**: Updates über Installationsprogramm oder Anwendung aktualisieren gleichzeitig den installierten privilegierten Dienst und die Task-Manager-Ersatzkopie.
- **Portable Version**: Zuerst die alte Version beenden, dann die Programmdatei ersetzen. Bei installiertem privilegiertem Dienst in der neuen Version unter „Einstellungen → Berechtigungen“ auf „Dienst aktualisieren“ klicken. Vor späteren manuellen Updates kann ein aktivierter Administratorstart deaktiviert und Argus beendet werden; anschließend die Datei ersetzen, die neue Programmdatei starten und die Option wieder aktivieren.

### Neue Funktionen

- **Administratorstart**: Nach einmaliger Freigabe startet Argus bei Anmeldung, über Verknüpfungen und angeheftete Symbole mit Administratorrechten; bei aktivierter Task-Manager-Ersetzung auch über Strg+Umschalt+Esc und das Taskleisten-Kontextmenü. Einstellungen und Einführung zeigen Freigabestatus und aktuelle Rechte an und erlauben das Deaktivieren oder Reparieren der Freigabe. ([#13](https://github.com/Ray-lea/Argus/issues/13))

### Fehlerbehebungen

- **Explorer**: Fehlgeschlagene Neustarts mit Standard- oder Administratorrechten, Desktopfehler nach dem Neustart und fehlende Rechte bei erneutem Neustart wurden behoben. Die Liste der Dateisperren bietet für den Explorer des aktuellen Desktops „Neu starten“, damit Desktop und Taskleiste nicht durch direktes Beenden verschwinden. ([#14](https://github.com/Ray-lea/Argus/issues/14), [#15](https://github.com/Ray-lea/Argus/issues/15))
- **CPU-Statistik**: Dauerhafte 100 % CPU-Auslastung bei nicht mehr aktualisierten Leerlaufzählern auf manchen Systemen wurde behoben. ([#12](https://github.com/Ray-lea/Argus/issues/12))
- **Diagramme**: Aufgestaute Daten bilden beim Wiederherstellen aus minimiertem Zustand oder Infobereich keine dicht gedrängten Spitzen mehr.
- **Prozessliste**: Der blinkende Textcursor nach einem Klick auf die Liste wurde entfernt. Direkte Suche durch Tippen und chinesische Eingabemethoden bleiben erhalten. ([#16](https://github.com/Ray-lea/Argus/issues/16))
- **Updateinstallation**: Prozesse, die die Programmdatei belegen, werden angezeigt; ein erneuter Versuch ist möglich. Fehlgeschlagene Schritte und Fehlercodes wurden ergänzt. Nicht lesbare Updatediagnoseprotokolle nach Freigabe mit einem anderen Administratorkonto wurden korrigiert.

### Bekannte Probleme

- **Codesignierung**: Veröffentlichte Programme haben noch keine Windows-Codesignatur. Windows SmartScreen kann beim ersten Start vor einem „unbekannten Herausgeber“ warnen; integrierte Updates werden weiterhin anhand ihrer Updatesignatur geprüft.

## [0.3.0](https://github.com/Ray-lea/Argus/compare/v0.2.0...v0.3.0) - 2026-10-22

Neu sind das Ausführen neuer Aufgaben, der Explorer-Neustart und das Kopieren der PC-Spezifikationen. Prozessgruppen und Leistungsansichten wurden erweitert und die Windows-10-Kompatibilität verbessert.

### Inkompatible Änderungen

- **Verlauf**: Nach dem Update beginnt die Verlaufsseite mit neuen Aufzeichnungen; die alte Datei des aktuellen Tages wird ersetzt. Zum Erhalten alter Aufzeichnungen muss zuvor der [Verlaufsordner gesichert](https://github.com/Ray-lea/Argus/blob/main/docs/guide/README.md#历史) werden.

### Hinweise zur Aktualisierung

- **Installierte Version**: Ein Update über Installationsprogramm oder Anwendung aktualisiert auch den privilegierten Dienst und die Task-Manager-Ersatzkopie.
- **Portable Version**: Die alte Version beenden und dann die Programmdatei ersetzen. Bei installiertem privilegiertem Dienst unter „Einstellungen → Berechtigungen“ auf „Dienst aktualisieren“ klicken und die Freigabe bestätigen.

### Neue Funktionen

- **Neue Aufgabe ausführen**: Ein neuer Zugang in der Symbolleiste öffnet Programme, Ordner, Dokumente oder Webadressen, auch mit Administratorrechten.
- **Explorer neu starten**: Im Kontextmenü des Windows-Explorers auf der Seite Prozesse steht „Neu starten“ zur Verfügung.
- **Tastenkombinationen**: Eine gedrückte Strg-Taste pausiert die Aktualisierung; Loslassen setzt sie fort. Alt+E beendet auf der Seite Prozesse den ausgewählten Prozess. Die Einstellungen bieten eine Übersicht der Tastenkombinationen.
- **Prozessgruppen**: Die App-Ansicht ist in Apps, Hintergrundprozesse und Windows-Prozesse unterteilt. Gruppenüberschriften sind einklappbar und zeigen den Gesamtverbrauch.
- **Schnellsuche**: Direktes Tippen auf der Seite Prozesse startet die Suche, auch mit chinesischen Eingabemethoden. Aktive Filter werden in der Akzentfarbe hervorgehoben.
- **PC-Spezifikationen**: Der Detailbereich bleibt standardmäßig geöffnet und ist in der Breite anpassbar. Ohne Prozessauswahl zeigt er die Hardwareausstattung, die ausgeblendet oder als Text kopiert werden kann.
- **CPU-Seite**: Aufteilung in Übersicht und Details; Kerndiagramme ordnen sich mit fließenden Übergängen entsprechend der Fenstergröße an.
- **GPU-Seite**: Aktive Engines werden automatisch angezeigt. Engines, Diagrammlayout und Höhe lassen sich auch manuell wählen.
- **Speicherzusammensetzung**: Geänderter Speicher wurde ergänzt. Die Symbole der Legende zeigen beim Überfahren Erläuterungen.
- **Akkuverlauf**: Verlauf zeigt zusätzlich Akkuladestand sowie Lade- und Entladeleistung zum Vergleich mit der Ressourcennutzung.
- **Anheften und Notizen**: Auf Dienste, Autostart und Verbindungen können Einträge per Kontextmenü angeheftet und mit Notizen versehen werden.
- **Fensteranimationen**: Minimieren, Wiederherstellen sowie Ein- und Ausblenden über den Infobereich verwenden eine Überblendung an Ort und Stelle.
- **Änderungshinweise**: Einstellungen und Dialoge verwenden ein einheitliches Layout, unterstützen englische Hinweise und passen die Dialoghöhe an das Fenster an.
- **Diagnoseprotokolle**: Ergänzt wurden fehlgeschlagene Schritte, Fehlercodes, relevante Pfade sowie CPU- und Grafikkartenmodelle für Fehlerberichte.
- **Info-Seite**: Ein Link zur Projekt-Homepage wurde ergänzt.

### Fehlerbehebungen

- **CPU-Statistik**: Zu niedrige Auslastungswerte bei kurzen Lasten wurden für Gesamt-CPU, logische Prozessoren und Prozesse behoben.
- **Windows 10**: Durchscheinender Desktop im Fensterhintergrund, leere Systemeinstellungsfenster und fehlende Datenträger-Lese-/Schreibwerte je Prozess wurden behoben.
- **Task-Manager-Ersetzung**: Fehlgeschlagene Aktivierung und falsche Meldungen über einen laufenden Start- oder Beendigungsvorgang wurden für bestimmte Umgebungen behoben. Hinweise auf blockierende Sicherheitssoftware wurden ergänzt.
- **Kernelsensoren**: Fehler bei Erkennung und Upgrade älterer PawnIO-Versionen sowie die Anzeige eines aktivierten Zustands trotz Treiberfehler wurden behoben.
- **Autostart-Apps**: Anzeige, Aktivierung und Deaktivierung paketierter Apps wurden vervollständigt; Namen und Status wurden korrigiert.
- **Programmstart**: Ein Fehler beim Öffnen des Argus-Fensters nach Freigabe mit einem anderen Administratorkonto oder bei beendetem Explorer wurde behoben.
- **Prozesserkennung**: Die App-Zuordnung von WebView2, versehentlich zusammengefasste gleichnamige Programme und falsche Gruppen bei pausierter Sortierung wurden korrigiert.
- **Chinesische Eingabe**: Das erste eingegebene Zeichen nach der Rückkehr zur Prozessseite geht nicht mehr verloren.
- **Diagramme**: Fortschreitendes Abschneiden am linken Rand, stockende Messpunkte beim Überfahren und springende Kurven nach einer Pause wurden behoben. Übergänge bei geänderter Obergrenze wurden verbessert.
- **Seitenlayout**: Layoutsprünge von Seiten und Dialogen beim Erscheinen von Bildlaufleisten wurden behoben.
- **Verlaufsspeicherung**: Bereits aufgezeichnete Daten gehen bei einem Lesefehler der Tagesdatei nicht mehr verloren.
- **Sicherheit**: Basiskomponenten wurden aktualisiert und die Beschränkungen für das Laden externer Inhalte in eingebetteten Seiten verstärkt.

### Leistung

- Flüssigeres Scrollen der Prozessliste, weniger wiederholte Hintergrundaktualisierungen und geringerer zugehöriger Ressourcenaufwand.

### Bekannte Probleme

- **Codesignierung**: Das offizielle Zertifikat ist beantragt. Windows SmartScreen kann beim ersten Start vor einem „unbekannten Herausgeber“ warnen.

Vielen Dank an @kostyanp95 für den Beitrag zum Akkuverlauf.

## [0.2.0](https://github.com/Ray-lea/Argus/compare/v0.1.0...v0.2.0) - 2026-10-18

Diese Version ergänzt Datenträger-Lese- und Schreibraten je Prozess und getrennte Aktualisierungsintervalle für minimierte und im Infobereich verborgene Fenster. Integrierte Updates und Tastenkombinationen wurden verbessert; Abstürze nach längerer Laufzeit auf manchen Rechnern und weitere Fehler wurden behoben. Bei installiertem privilegiertem Dienst ist vorab der Abschnitt „Inkompatible Änderungen“ zu beachten.

### Inkompatible Änderungen

- **Update des privilegierten Dienstes erforderlich**: Die Kommunikation zwischen Argus und dem privilegierten Dienst wurde geändert. Der Dienst aus 0.1.0 ist mit der neuen Version inkompatibel. Ohne installierten Dienst besteht keine Auswirkung.
- **Update über das Installationsprogramm**: Dies umfasst integrierte Updates und das direkte Ausführen des Installationsprogramms. Es beendet zunächst noch laufende alte Argus-Kopien, die über die Task-Manager-Ersetzung gestartet wurden, und aktualisiert den privilegierten Dienst mit derselben Administratorfreigabe ohne weitere Schritte. Ist „Task-Manager ersetzen“ aktiviert, bleibt die Option aktiviert; nach erfolgreichem Update öffnet Strg+Umschalt+Esc die neue Version.
- **Update der portablen Version**: Bei installiertem privilegiertem Dienst muss nach Austausch der Hauptprogrammdatei auch der Dienst manuell aktualisiert werden. Dazu im Hinweis der neuen Argus-Version oder unter „Einstellungen → Berechtigungen“ auf „Dienst aktualisieren“ klicken und zustimmen. Dasselbe Vorgehen gilt bei einem fehlgeschlagenen Dienstupdate durch das Installationsprogramm. Bis zum Dienstupdate fehlen dienstabhängige Netzwerk-, Datenträger- und CPU-Sensordaten; Verwaltungsaktionen mit erhöhten Rechten können fehlschlagen. Bei aktivierter Option „Task-Manager ersetzen“ kann Strg+Umschalt+Esc noch die alte Version öffnen. Das Dienstupdate aktualisiert auch das Ersatzprogramm und lässt die Ersetzung aktiviert.

### Neue Funktionen

- **Datenträgerzugriffe je Prozess**: Die Prozessseite ergänzt die sortierbaren Spalten „Datenträger lesen“ und „Datenträger schreiben“ mit den pro Sekunde gelesenen und geschriebenen Datenmengen. App-Gruppen zeigen die Summe ihrer Mitglieder; die Werte erscheinen auch im Detailbereich. I/O-Spalten umfassen zusätzlich Netzwerk- und Gerätezugriffe und werden getrennt gezählt. Windows 11 24H2 und neuer benötigen keine zusätzlichen Rechte; ältere Systeme benötigen den privilegierten Dienst oder Administratorrechte. Dies wurde in den Freigabehinweisen des Dienstes ergänzt. Fehlende Messwerte erscheinen als „—“, der Grund beim Überfahren. In schmalen Fenstern bleiben beide Spalten erhalten; die Tabelle lässt sich horizontal scrollen, während Name links fixiert bleibt. Daher werden Threads, Handles, PID und I/O früher als bisher ausgeblendet.
- **Updates in der Anwendung**: Ab dieser Version erscheinen nach Download und erfolgreicher Signaturprüfung rechts in der Titelleiste eine Update-Schaltfläche, am Symbol im Infobereich ein Punkt und unten rechts einmalig pro Version ein Hinweis. Die portable Version meldet neue Versionen bereits beim Erkennen und lädt sie nicht automatisch herunter. Der Updatedialog zeigt nach Kategorien die Änderungen aller Versionen zwischen aktueller und neuer Version samt Anzahl der Änderungen je Version. Änderungshinweise werden ebenso wie Installationspakete per Signatur geprüft. Im Kontextmenü der Update-Schaltfläche steht „Für diese Version nicht erneut erinnern“. Heruntergeladene Updates bleiben nach einem Argus-Neustart verfügbar.
- **Änderungen nach dem Update anzeigen**: Beim ersten Öffnen des Fensters nach einem Update erscheinen die Neuerungen. „Neuerungen nach dem Update anzeigen“ lässt sich unter „Einstellungen → Updates“ deaktivieren; anschließend erscheint nur unten rechts ein Hinweis auf das abgeschlossene Update. „Änderungen dieser Version anzeigen“ auf derselben Seite öffnet die Hinweise jederzeit.
- **Aktualisierung im Hintergrund**: Für Minimierung und Ausblenden im Infobereich sind getrennte Intervalle von 0,5 bis 30 s einstellbar, standardmäßig 5 s, unter „Einstellungen → Daten und Verlauf“ sowie in der Einführung. Unter 5 s wird vorab auf höhere CPU-Last und höheren Energieverbrauch hingewiesen; über 5 s wird auch der Verlauf gröber aufgezeichnet. Bei Rückkehr in den Vordergrund erfolgt sofort eine Aktualisierung. Ist das Hintergrundintervall länger als im Vordergrund, erscheint unten rechts einmalig ein Hinweis mit direktem Zugang zu den Einstellungen und einer Option zum Unterdrücken weiterer Hinweise.
- **Tastenkombinationen einstellen**: Eine Tastenkombination kann deaktiviert werden, ohne sie zu löschen; erneutes Aktivieren stellt sie wieder her. Die Aufnahme zeigt gedrückte Tasten sofort an, erklärt unzulässige Kombinationen und bietet „Standard wiederherstellen“. Konfliktanfällige Kombinationen wie Strg+Buchstabe oder Alt+Buchstabe werden erst nach Bestätigung gespeichert. Bei Belegung durch ein anderes Programm ist ein direkter Wiederholungsversuch möglich; nach dessen Ende versucht Argus die Registrierung beim nächsten Anzeigen des Fensters automatisch erneut.
- **CPU-Betriebszeit**: Sekundengenaue Anzeige, etwa „5 Tage 14:03:07“, die bei jeder Aktualisierung fortgeschrieben wird.

### Fehlerbehebungen

- **Abstürze**: Mögliche Abstürze von Argus nach einiger Laufzeit auf Rechnern ohne verfügbare Intel-Grafik wurden behoben.
- **Verlorene Einstellungen**: Ein Stromausfall oder erzwungener Neustart kurz nach dem Speichern führte zuvor zu verlorenen Einstellungen, erneut angezeigter Einführung und Rückkehr zur Systemsprache. Dies wurde behoben.
- **Autostart**: Der automatische Start entzieht dem aktiven Fenster nicht mehr den Fokus. Zuvor gelangten Tastatureingaben bis zum nächsten Klick an eine andere Stelle in das unsichtbare Argus-Fenster. Im Infobereich verborgen wird die Oberfläche nicht mehr fortlaufend gezeichnet; CPU- und GPU-Last sinken deutlich, der Speicherbedarf um mehr als die Hälfte.
- **Autostarteintrag**: Wird „Bei Windows-Anmeldung starten“ in einem über Strg+Umschalt+Esc geöffneten Argus aktiviert, verweist der Eintrag auf die installierte Version. Einträge, die bereits auf die Task-Manager-Ersatzkopie zeigen, werden beim einmaligen Start der neuen Version etwa über das Startmenü automatisch korrigiert.
- **Hintergrundaktualisierung**: Das tatsächliche Intervall bei minimiertem oder verborgenem Fenster entspricht der Einstellung. Zuvor wurden aus 5 s oft 6 s, gelegentlich 10 s. Mit Standardrechten und privilegiertem Dienst verschwinden die Netzwerkspalten auf der Prozessseite nach Wiederherstellung aus Infobereich oder Minimierung nicht mehr kurzzeitig.
- **Fenster**: Im maximierten Zustand erscheinen oben rechts keine zwei überlappenden Schaltflächengruppen mehr. Das Fenster lässt sich auch mit dem Zeiger direkt am oberen Bildschirmrand ziehen.
- **Installation und Deinstallation**: „Schließen“ auf der Abschlussseite, „Abbrechen“ bei der Deinstallationsbestätigung und die Kontrollkästchenzeile sind nun zentriert; zuvor waren sie leicht nach links versetzt.
- **Update mit Installationsprogramm**: Beim direkten Update werden auch noch laufende alte Argus-Kopien beendet, die über Strg+Umschalt+Esc oder alte Autostarteinträge gestartet wurden. Dadurch werden spätere Starts nicht weiter an die alte Version übergeben; vorheriges Beenden über den Infobereich entfällt.
- **Deinstallation**: Ein über Strg+Umschalt+Esc gestartetes Argus wird ebenfalls beendet. Zuvor blieb es im Hintergrund aktiv und schrieb nach Auswahl von „Auch Einstellungen, Verlauf und Protokolle löschen“ und Neuinstallation alte Einstellungen zurück; die Einführung erschien beim ersten Start nicht. Können Daten nicht vollständig entfernt werden, listet das Deinstallationsprogramm die verbleibenden Ordner auf.
- **Einführung**: Beim Start von Argus mit gedrücktem Strg+Umschalt+Esc schließt das gehaltene Esc nicht mehr sofort die Einführung und markiert sie als abgeschlossen.
- **Aufnahme von Tastenkombinationen**: Das Drücken der aktuellen Kombination oder längeres Halten einer Taste blendet das Fenster während der Aufnahme nicht mehr versehentlich aus. Bei anderen Tastaturlayouts als US stimmen aufgenommene und tatsächlich auslösende Tasten überein. Umschalt+Rücktaste und Win+Rücktaste löschen die Kombination nicht mehr irrtümlich.
- **Zulässige Tastenkombinationen**: Neue Kombinationen müssen Strg, Alt oder Win enthalten, ausgenommen einzelne Tasten wie F13–F24. Kombinationen wie Umschalt+Buchstabe, die das Tippen beeinträchtigen, und übliche Bearbeitungs- oder Systembefehle wie Strg+C und Alt+F4 werden abgewiesen. Gespeicherte Kombinationen bleiben unverändert.
- **Tastenkombinationen**: Manche Ziffernblocktasten werden nicht mehr als andere Tasten registriert. Scheitert das Speichern beim Ändern, entsteht keine zusätzliche, nicht mehr aufhebbare Tastenkombination. Registrierungsfehler werden in der Oberflächensprache erklärt. In der Einführung werden bei belegten oder unzulässigen Kombinationen lediglich der Grund und die Änderungsmöglichkeit in den Einstellungen angezeigt.
- **Task-Manager ersetzen**: Beim Dienstupdate bleibt die Ersetzung aktiv und die verwendete Argus-Kopie wird mit aktualisiert. Das vorherige Wiederherstellen des Windows-Task-Managers und manuelle erneute Aktivieren entfallen. Bei fehlgeschlagener Aktivierung werden der abgewiesene Schritt und mögliche Ursachen genannt, etwa blockierte Registrierungsänderungen durch Sicherheitssoftware. „Von einem anderen Programm eingerichtet“ nennt die konkreten Konfigurationen. Nach Deaktivierung behindern auch andere Task-Manager-Ersatzprogramme wie Process Explorer das Aktualisieren oder Deinstallieren des Dienstes nicht mehr. Fehlgeschlagene Aktivierungen hinterlassen keine überflüssige Argus-Kopie im Dienstordner. Alte Kopien werden bei Upgrades, Ein- oder Ausschalten der Ersetzung sowie Aktualisierung oder Deinstallation des Dienstes bereinigt, sofern sie nicht laufen.
- **Prozesse**: „Details anzeigen“ ohne Auswahl fordert nun zur Auswahl eines Listeneintrags auf; zuvor geschah nichts. Bei geöffnetem Kontextmenü oder Bestätigungsdialog zeigt die Statusleiste nicht mehr „Die Sortierung wurde angehalten, während sich der Zeiger über der Liste befindet“, und die Liste sortiert sich nicht beim Öffnen der Bestätigung neu. Ein Seitenwechsel per Tastenkombination bei über der Liste ruhendem Zeiger hinterlässt den Hinweis nicht mehr auf anderen Seiten.
- **Diagramme**: Der Messpunkt stockt nicht mehr, wenn der Zeiger nahe am rechten Diagrammrand liegt.
- **Updatekanal wechseln**: Beim Wechsel zwischen Stabil und Beta während Prüfung oder Download wird im neuen Kanal erneut geprüft, statt die Version des alten Kanals anzubieten.
- **Diagnoseprotokoll**: Fehler enthalten den Systemfehlercode statt des allgemeinen operation failed, das zuvor auf nicht englischen Systemen fast durchgängig erschien. Beim Start wird auch der Grund einer fehlgeschlagenen Dienstverbindung protokolliert, etwa inkompatible Version oder Zeitüberschreitung.

### Leistung

- **Prozessseite**: Die kleinen Auslastungsbalken je logischem Prozessor in der Systemübersicht benötigen weniger Zeichenaufwand. Bei laufenden Kurven übermittelt die Oberfläche pro Frame etwa ein Drittel weniger Daten an die Grafikkarte. Da die WebView2-Zugriffe in den I/O-Spalten überwiegend daraus stammen, sinken auch diese Werte.

### Bekannte Einschränkungen

- **Datenträgerzugriffe**: Die Lesemethode für Windows 11 24H2 und neuer wurde nur unter Build 26300 geprüft. Ältere Systeme lesen über den privilegierten Dienst oder Administratorrechte; auf diesen Systemen steht die Prüfung noch aus. Bei dauerhaftem „—“ in beiden Spalten kann eine Meldung mit Diagnoseprotokoll eingereicht werden; der Grund des Lesefehlers wird darin erfasst.
- **Codesignierung**: Das Programm ist noch nicht codesigniert. Windows SmartScreen kann beim ersten Start vor einem „unbekannten Herausgeber“ warnen.
- **Weitere Einschränkungen**: Weitere Angaben stehen im Dokument [Bekannte Probleme](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md) auf GitHub.

## 0.1.0 - 2026-10-16

Die erste öffentliche Version von Argus.

### Neue Funktionen

- **Prozesse**: Anzeige nach App, als Prozessbaum oder Liste; bei Namenssortierung in „Apps“ und „Hintergrundprozesse“ aufgeteilt. Je Prozess werden CPU, Arbeitsspeicher, I/O und GPU angezeigt, mit privilegiertem Dienst oder Administratorrechten zusätzlich Netzwerkverkehr. Suche und Beenden einzelner Prozesse oder ganzer Prozessbäume sind möglich. Der Detailbereich zeigt Befehlszeile, Benutzer, Integritätsstufe und weitere Angaben; bei App-Gruppierung die Gesamtsumme der App.
- **Priorität und Effizienzmodus**: Über das Kontextmenü einstellbar oder als Regeln, die bei jedem Programmstart automatisch angewendet werden.
- **Leistung**: CPU (je Kern, Leistungs- und Effizienzkerne, Takt, Temperatur, Leistungsaufnahme, Leerlaufzustände), Arbeitsspeicher, Datenträger, Netzwerk, GPU (mit Leistungsaufnahme in Watt für NVIDIA, AMD und Intel) und Akku. Kurven laufen mit der Bildwiederholrate des Displays und passen sich der Fenstergröße an. Selten benötigte Angaben stehen in ausklappbaren Karten.
- **Optionale Kernelsensoren**: Nach Aktivierung in Einstellungen oder Einführung werden interne CPU-Sensoren über den privilegierten Dienst und den offiziellen PawnIO-Treiber gelesen: Temperatur und Spannung je Kern, Leistungsaufnahme und Limitstatus, AMD-SMU-Daten sowie Lüfterdrehzahlen, Spannungen und Temperaturen unterstützter Mainboards.
- **Dienste und Autostart-Apps**: Dienste starten, stoppen und ihren Starttyp ändern; Autostart-Apps über denselben Schalter wie im System-Task-Manager aktivieren oder deaktivieren.
- **Verbindungen und Ports**: TCP- und UDP-Endpunkte samt zugehörigen Prozessen, exakte Portsuche und Schließen von IPv4-TCP-Verbindungen.
- **Dateisperren**: Dateien oder Ordner auf das Fenster ziehen, um verwendende Prozesse mit Restart Manager und Handle-Auflistung zu finden, einschließlich der aktuellen Prozessverzeichnisse.
- **Verlauf**: Die Ressourcennutzung der letzten 7 Tage wird aufgezeichnet. Ein Klick auf einen Zeitpunkt zeigt die damals am stärksten auslastenden Prozesse.
- **Infobereich**: Die globale Tastenkombination Strg+Alt+Esc blendet das Fenster ein oder aus; verborgen ist der Ressourcenverbrauch sehr gering.
- **Optionaler privilegierter Dienst**: Nach einmaliger Administratorfreigabe kann die Oberfläche mit Standardrechten Systemprozesse verwalten, Regeln automatisch anwenden, Netzwerkverkehr je Prozess messen, Dienste steuern und Verbindungen schließen.
- **Task-Manager-Ersetzung**: Strg+Umschalt+Esc und andere Aufrufwege können Argus öffnen. Ein eigenständiges Wiederherstellungsprogramm arbeitet unabhängig von Oberfläche und Dienst.
- **Updates in der Anwendung**: Kanäle Stabil und Beta, optionaler GitHub-Proxy; installiert werden ausschließlich Versionen mit erfolgreich geprüfter Signatur.
- **Aussehen**: Die Symbolleiste dient zugleich als Titelleiste; die Navigation liegt darunter oder links im Fenster. Mica-Hintergrund und Systemakzentfarbe; helles und dunkles Design folgen standardmäßig Windows und lassen sich in den Einstellungen festlegen.
- **Installation und Deinstallation**: Einseitiges Installationsprogramm im hellen oder dunklen Windows-Modus. Ältere installierte Versionen werden direkt aktualisiert; beim Deinstallieren können Einstellungen, Verlauf und Protokolle mit entfernt werden.
- **Sprachen und Einführung**: Oberfläche, Infobereich und Installationsprogramm unterstützen vereinfachtes Chinesisch und Englisch und folgen standardmäßig der Windows-Anzeigesprache. Beim ersten Start führt eine Einführung durch Sprache, Anzeige und Aktualisierung, Infobereich, Updates und weitere übliche Einstellungen. Dabei lassen sich auch der optionale privilegierte Dienst installieren, Kernelsensoren aktivieren oder der System-Task-Manager ersetzen.

### Bekannte Einschränkungen

- Das Programm ist noch nicht codesigniert. Windows SmartScreen kann beim ersten Start vor einem „unbekannten Herausgeber“ warnen.
- Interne CPU-Sensoren erfordern aktivierte Kernelsensoren (privilegierter Dienst und PawnIO-Treiber). Unterstützte Prozessoren und Mainboards stehen unter [Hardwaredaten](https://github.com/Ray-lea/Argus/blob/main/docs/guide/hardware-data.md#消费级硬件兼容范围).
- Weitere Einschränkungen stehen unter [Bekannte Probleme](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md).
