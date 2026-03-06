# WASI-USB & RUSB Integration Update: PS5-Maze

Dit document beschrijft de technische implementatie en de noodzakelijke aanpassingen voor het compileren van de `ps5-maze` applicatie naar een WebAssembly component (WASI) met directe USB-toegang via de `rusb` bibliotheek.

## 1. Doelstelling
Het hoofddoel van dit project is het demonstreren van hardware-interactie (USB Gamepads) vanuit een sandboxed WebAssembly omgeving. Hiervoor gebruiken we de nieuwe **WASI-USB** standaard in combinatie met een aangepaste versie van de populaire Rust bibliotheek `rusb`.

## 2. Architectuur & Componenten

### rusb-wasi
Voor dit project is een fork van `rusb` ontwikkeld (`rusb-wasi`) die niet langer afhankelijk is van native OS API's (zoals IOKit op macOS of udev op Linux), maar communiceert met de `libusb-wasi` interface. Deze bibliotheek fungeert als de bridge tussen de Rust applicatie en de WASI-host.

### libusb-wasi
Dit is de onderliggende C-bibliotheek die gecompileerd is naar WASM met de `wasi-sdk`. Het implementeert de standaard `libusb` API, maar stuurt de calls door naar de WASI-USB host-imports, ontwikkeld door Robbe Leroy.

### WASI-USB Host
De applicatie draait op de `usb-wasi-host` runtime. Deze host-omgeving vangt de USB-calls van de WASM-module op en voert deze uit op de fysieke USB-bus van de host-machine (voldoende rechten zoals `sudo` zijn hierbij vereist op macOS/Linux).

## 3. Compilatie Workflow

Het compileren naar de nieuwe `wasm32-wasip2` target vereist een specifieke omgeving waarin de C-headers en bibliotheken van `libusb-wasi` vindbaar zijn.

### Noodzakelijke Omgevingsvariabelen
Voor een succesvolle build moeten de volgende variabelen worden ingesteld, zodat `cargo` de WASM-versie van `libusb` kan linken:

*   `SYSROOT`: Pad naar de `wasi-sysroot` die de gecompileerde `libusb` bevat.
*   `PKG_CONFIG_LIBDIR`: Verwijst naar de `.pc` bestanden in de sysroot.
*   `LIBUSB_STATIC=1`: Forceert het statisch linken van de bibliotheek in de WASM component.

**Voorbeeld build-commando:**
```bash
export SYSROOT=[pad-naar-sysroot]
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR=$SYSROOT/usr/lib/pkgconfig
export LIBUSB_STATIC=1

cargo build --target wasm32-wasip2 --release --bin ps5-maze
```

## 4. Code Aanpassingen voor WASI-USB v2

Om `ps5-maze` compatibel te maken met de nieuwste WASI-USB specificaties en `rusb-wasi`, zijn de volgende code-technische wijzigingen doorgevoerd:

*   **Context Management**: In v2 van de bridge is de `rusb::Context` explicieter geworden. We initialiseren een context die de `libusb-wasi` backend aanstuurt, wat essentieel is voor correct resource management binnen de WASM-sandbox.
*   **Asynchrone Polling via Timeouts**: Omdat WASI (nog) geen volledige multi-threading ondersteunt zoals native OS'en, is de `read_interrupt` call aangepast met een strikte timeout (50ms). Dit voorkomt dat de WASM-module de host "blockt" en staat toe dat de game-loop (Ghost AI, timers) blijft draaien, zelfs als er geen controller-input is.
*   **Explicit Interface Detaching**: Hoewel de host veel regelt, bevat de code nu expliciete checks voor `has_kernel_driver` en `detach_kernel_driver`. Dit is cruciaal voor de stabiliteit van de bridge wanneer de host-machine de controller al als een standaard HID device heeft geclaimd.
*   **Memory Safety & Lifetimes**: De integratie met `rusb-wasi` vereiste striktere lifetime management voor `DeviceHandle` en `Context` om te garanderen dat USB-bronnen netjes worden vrijgegeven wanneer de WASM-module herstart of de game eindigt.

## 5. Implementatie Details in `ps5-maze`

### USB Interface Discovery
In tegenstelling tot native systemen, waar de controller vaak direct als een HID-device wordt herkend, communiceert de WASM-app rechtstreeks op USB-niveau:
1.  **Device Zoeken**: Scan de bus op `054c:0ce6` (DualSense) of `045e:02ea` (Xbox).
2.  **Interface Claimen**: Voor de PS5-controller is specifiek **Interface 03** geclaimd. Dit is de interface die de HID-rapporten via een Interrupt Endpoint verstuurt.
3.  **Interrupt Transfers**: Er wordt gebruik gemaakt van `handle.read_interrupt` met een timeout van 50ms. Dit zorgt voor een responsieve game-loop zonder de CPU volledig te belasten.

### Controller Mapping
De ruwe USB-data (HID reports) wordt handmatig geparsed in `src/lib.rs`. 
*   **Analoge Sticks**: Byte 1 (LS X) en Byte 2 (LS Y) worden genormaliseerd naar een float `-1.0` tot `1.0`.
*   **Deadzone**: Een deadzone van `0.5` is geïmplementeerd om onbedoelde bewegingen (drift) te voorkomen.
*   **Polling**: Om de beweging vloeiend te houden, wordt de invoer vaker verwerkt dan de rendering.

### Rendering & Ghost AI
*   **ANSI Escapes**: De game gebruikt ANSI escape codes (`\x1B[H`, `\x1B[2J`) voor rendering in de terminal. Dit is native ondersteund door de WASI-host.
*   **Ghost AI**: Implementatie van Blinky, Pinky en Clyde met hun originele targeting-logica, maar geoptimaliseerd voor de single-threaded WASM executie.

## 5. Vergelijking met Vorige Versie

In vergelijking met de initiële `xbox-maze` implementatie zijn de volgende verbeteringen doorgevoerd:

*   **Authentieke Ghost AI**: De spoken bewegen niet langer willekeurig. Blinky, Pinky en Clyde hebben elk hun eigen jacht-stijl (targeting), preventie van 180-graden bochten op rechte stukken, en een 'vlucht'-modus wanneer Pac-Man een power pellet eet.
*   **Tile Persistence**: Een cruciaal mechanisme waarbij spoken de onderliggende tegel (zoals een puntje) onthouden en terugplaatsen. In de vorige versie "aten" de spoken het speelveld leegg.
*   **PS5 DualSense Integratie**: Waar de vorige versie enkel Xbox-controllers ondersteunde, kan deze versie specifiek de HID reports van de DualSense (Interface 03) parsen.
*   **Analoge Besturing**: Toevoeging van joystick-ondersteuning met deadzone-detectie, naast de standaard D-pad.
*   **Real-time Debug HUD**: Een dashboard boven het spel dat live de ruwe USB-inputs (stick percentages en button states) toont, wat essentieel is voor de validatie van de WASI-USB bridge.
*   **Gameplay Polish**: Implementatie van levens, een score-systeem, "Frightened Mode" (waarbij je spoken kunt vangen) en een win-conditie.

## 6. Resultaat
De applicatie bewijst dat complexe, real-time interacties mogelijk zijn binnen WASI-omgevingen met de juiste abstractielagen (`rusb` -> `libusb-wasi` -> `WASI-USB`). Dit opent de deur voor WebAssembly toepassingen die directe controle vereisen over industriële apparatuur, meetinstrumenten of (zoals hier) randapparatuur voor gaming.
