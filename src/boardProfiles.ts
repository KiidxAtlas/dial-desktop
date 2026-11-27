// What Dial knows about common boards: what they can do, their pins, safe wiring for first
// projects, and console commands worth trying. Pure data and lookups, so the Boards panel and
// its tests share one source. Pin facts come from the vendors' references (ESP-IDF GPIO tables,
// the Flipper Zero firmware's pin list, the Raspberry Pi Pico and Arduino Uno datasheets).
import type { Board, Ending } from './boardConsole';
import models from './boardCatalog.json' with { type: 'json' };

/** `power` and `ground` are supply pins; `caution` works but has a catch; `reserved` is taken by the board itself. */
export type PinKind = 'power' | 'ground' | 'io' | 'caution' | 'reserved';
export type Pin = { id: string; label: string; kind: PinKind; number?: number; functions: string[]; note?: string };
/** One wire: the part's lead, the board pin it goes to, and anything in between. */
export type Wire = { part: string; pin: string; via?: string };
export type Recipe = { id: string; title: string; parts: string; wires: Wire[]; steps: string[]; goal: string; try?: string[] };
export type Command = { label: string; text: string; raw?: boolean; note?: string };
export type Profile = {
  id: string;
  name: string;
  summary: string;
  /** Signal voltage on the pins. */
  logic: string;
  baud: number;
  ending: Ending;
  /** `row`: one numbered header; `dual`: two facing header columns (pins 1..n/2 down the left); `list`: grouped by role. */
  layout: 'row' | 'dual' | 'list';
  pinNote: string;
  abilities: string[];
  commands: Command[];
  /** Console commands can drive pins (`{pin}` is the pin label). */
  pinCommands?: { read: string; high: string; low: string };
  tips: string[];
  pins: Pin[];
  recipes: Recipe[];
};

// ---- ESP32 family ---------------------------------------------------------------------------
type EspSpec = {
  id: string; name: string; summary: string; abilities: string[];
  gpios: number[];
  adc: Record<number, string>;
  strap: number[]; usb: Record<number, string>; flash: number[]; uart: Record<number, string>;
  extra?: Record<number, { functions?: string[]; note?: string; kind?: PinKind }>;
  boot: number;
  pick: { led: number; button: number; analog: number; sda: number; scl: number; tx: number; rx: number };
};

function espPins(s: EspSpec): Pin[] {
  const supply: Pin[] = [
    { id: '3V3', label: '3V3', kind: 'power', functions: ['3.3 V supply out'], note: 'Powers sensors and modules. About 500 mA total for the whole board, radio included.' },
    { id: '5V', label: '5V', kind: 'power', functions: ['5 V from USB'], note: 'Straight from USB. Never connect it to a GPIO pin: they are 3.3 V only.' },
    { id: 'GND', label: 'GND', kind: 'ground', functions: ['Ground'], note: 'Every part you connect needs a wire to ground.' },
    { id: 'EN', label: 'EN', kind: 'caution', functions: ['Chip enable / reset'], note: 'Pulling it low resets the chip. The RST button does this.' },
  ];
  return supply.concat(s.gpios.map(n => {
    const functions: string[] = [];
    const notes: string[] = [];
    let kind: PinKind = 'io';
    if (s.adc[n]) functions.push(`Analog in (${s.adc[n]})`);
    if (s.uart[n]) { functions.push(s.uart[n]); notes.push('Carries the boot log and console on boards without native USB.'); kind = 'caution'; }
    if (s.usb[n]) { functions.push(s.usb[n]); notes.push('Wired to the USB port; using it as GPIO cuts the USB console.'); kind = 'reserved'; }
    if (s.flash.includes(n)) { functions.push('SPI flash'); notes.push('Connected to the flash memory. Do not use.'); kind = 'reserved'; }
    if (s.strap.includes(n) && kind === 'io') {
      functions.push('Strapping pin');
      notes.push(`Read at reset to pick the boot mode${n === s.boot ? '; this is the BOOT button' : ''}. Fine for outputs, but keep anything that pulls it high or low off it at power-up.`);
      kind = 'caution';
    }
    const extra = s.extra?.[n];
    if (extra?.functions) functions.push(...extra.functions);
    if (extra?.note) notes.push(extra.note);
    if (extra?.kind) kind = extra.kind;
    if (!functions.length) functions.push('Digital in/out, PWM');
    else if (kind !== 'reserved') functions.push('Digital in/out');
    return { id: `GPIO${n}`, label: `${n}`, kind, number: n, functions, note: notes.join(' ') || undefined };
  }));
}

function espRecipes(s: EspSpec): Recipe[] {
  const p = s.pick, g = (n: number) => `GPIO${n}`;
  return [
    { id: 'led', title: 'Blink an LED', parts: 'LED, 220–330 Ω resistor',
      wires: [{ part: 'LED long leg (+)', pin: g(p.led), via: '220 Ω resistor' }, { part: 'LED short leg (−)', pin: 'GND' }],
      steps: ['The resistor limits the current; without it the LED or pin can burn out.', `Drive GPIO${p.led} high to light it, low to turn it off.`],
      goal: `blink an external LED on GPIO${p.led} once a second and print the state over serial` },
    { id: 'button', title: 'Read a button', parts: 'Push button',
      wires: [{ part: 'Button, one side', pin: g(p.button) }, { part: 'Button, other side', pin: 'GND' }],
      steps: [`Turn on GPIO${p.button}'s internal pull-up: the pin reads 1 when released and 0 while pressed.`, 'No resistor needed with the pull-up.'],
      goal: `read a push button on GPIO${p.button} (to ground, internal pull-up, debounced) and print pressed/released over serial` },
    { id: 'analog', title: 'Read a potentiometer or analog sensor', parts: '10 kΩ potentiometer (or any 0–3.3 V sensor)',
      wires: [{ part: 'Outer leg', pin: '3V3' }, { part: 'Middle leg (wiper)', pin: g(p.analog) }, { part: 'Other outer leg', pin: 'GND' }],
      steps: [`GPIO${p.analog} is ${s.adc[p.analog]}. Keep the voltage between 0 and 3.3 V.`, 'Print readings as value=123 and the console charts them.'],
      goal: `read the ADC on GPIO${p.analog} ten times a second and print it as raw=<value> mv=<millivolts>` },
    { id: 'i2c', title: 'Connect an I²C sensor or display', parts: 'I²C module (BME280, SSD1306 OLED, …)',
      wires: [{ part: 'VCC / VIN', pin: '3V3' }, { part: 'GND', pin: 'GND' }, { part: 'SDA', pin: g(p.sda) }, { part: 'SCL', pin: g(p.scl) }],
      steps: ['Most breakout boards already have pull-up resistors.', 'Any free GPIO can be SDA or SCL; these two are a safe choice.', 'Scan the bus first to learn the module’s address.'],
      goal: `scan the I2C bus on SDA GPIO${p.sda} and SCL GPIO${p.scl} and print every address that answers` },
    { id: 'uart', title: 'Talk to another serial device', parts: 'GPS module, another board, or a USB-UART adapter',
      wires: [{ part: 'Device RX', pin: g(p.tx) }, { part: 'Device TX', pin: g(p.rx) }, { part: 'Device GND', pin: 'GND' }],
      steps: ['TX goes to RX and RX to TX: each side talks into the other’s ear.', 'Only connect power if the device is not powered on its own. 5 V devices need a level shifter on their TX line.'],
      goal: `open a second UART on TX GPIO${p.tx} and RX GPIO${p.rx} at 9600 baud and forward everything it receives to the USB console` },
  ];
}

const ESP_TIPS = [
  'Connect, then press Reset to see the boot log: the first lines name the chip.',
  'Garbled text usually means the wrong baud rate. ESP32 boot ROMs print at 115200 (74880 on the original ESP32 with a 26 MHz crystal).',
  'If uploads fail, hold BOOT, tap RST, then release BOOT to enter the download mode.',
];
const ESP_SPECS: EspSpec[] = [
  { id: 'esp32c6', name: 'ESP32-C6', summary: 'RISC-V microcontroller with Wi-Fi 6, Bluetooth LE 5 and an 802.15.4 radio for Thread, Zigbee and Matter.',
    abilities: ['Wi-Fi 6 (2.4 GHz)', 'Bluetooth LE 5', 'Thread / Zigbee', 'Matter', '7 analog inputs', 'PWM on any pin', 'I²C, SPI, UART', 'Low-power core', 'Native USB console'],
    gpios: Array.from({ length: 24 }, (_, i) => i),
    adc: Object.fromEntries([0, 1, 2, 3, 4, 5, 6].map(n => [n, `ADC1_CH${n}`])),
    strap: [4, 5, 8, 9, 15], usb: { 12: 'USB D−', 13: 'USB D+' }, flash: [], uart: { 16: 'UART0 TX', 17: 'UART0 RX' }, boot: 9,
    extra: { 8: { functions: ['RGB LED (DevKitC-1)'], note: 'The DevKitC-1’s RGB LED is an addressable WS2812: it needs the led_strip driver, not a plain high/low.' },
      10: { note: 'Not led out on chips with built-in flash (ESP32-C6FH4/FH8).', kind: 'caution' }, 11: { note: 'Not led out on chips with built-in flash (ESP32-C6FH4/FH8).', kind: 'caution' } },
    pick: { led: 18, button: 19, analog: 2, sda: 23, scl: 22, tx: 20, rx: 21 } },
  { id: 'esp32c3', name: 'ESP32-C3', summary: 'Small RISC-V microcontroller with Wi-Fi and Bluetooth LE 5.',
    abilities: ['Wi-Fi (2.4 GHz)', 'Bluetooth LE 5', '6 analog inputs', 'PWM on any pin', 'I²C, SPI, UART', 'Native USB console'],
    gpios: Array.from({ length: 22 }, (_, i) => i),
    adc: { 0: 'ADC1_CH0', 1: 'ADC1_CH1', 2: 'ADC1_CH2', 3: 'ADC1_CH3', 4: 'ADC1_CH4', 5: 'ADC2_CH0' },
    strap: [2, 8, 9], usb: { 18: 'USB D−', 19: 'USB D+' }, flash: [12, 13, 14, 15, 16, 17], uart: { 21: 'UART0 TX', 20: 'UART0 RX' }, boot: 9,
    extra: { 8: { functions: ['RGB LED (DevKitM-1)'], note: 'On DevKitM-1 boards this drives an addressable WS2812 LED.' }, 5: { note: 'ADC2 is unreliable while Wi-Fi is on; prefer GPIO0–4 for analog.' } },
    pick: { led: 7, button: 10, analog: 3, sda: 4, scl: 6, tx: 0, rx: 1 } },
  { id: 'esp32s3', name: 'ESP32-S3', summary: 'Dual-core Xtensa microcontroller with Wi-Fi, Bluetooth LE 5, vector instructions for AI, and USB OTG.',
    abilities: ['Wi-Fi (2.4 GHz)', 'Bluetooth LE 5', 'USB device/host', '20 analog inputs', 'Touch sensing', 'Camera and LCD interfaces', 'PWM on any pin', 'I²C, SPI, UART'],
    gpios: [...Array.from({ length: 22 }, (_, i) => i), ...Array.from({ length: 23 }, (_, i) => i + 26)],
    adc: Object.fromEntries([...Array.from({ length: 10 }, (_, i) => [i + 1, `ADC1_CH${i}`]), ...Array.from({ length: 10 }, (_, i) => [i + 11, `ADC2_CH${i}`])]),
    strap: [0, 3, 45, 46], usb: { 19: 'USB D−', 20: 'USB D+' }, flash: [26, 27, 28, 29, 30, 31, 32], uart: { 43: 'UART0 TX', 44: 'UART0 RX' }, boot: 0,
    extra: { ...Object.fromEntries([33, 34, 35, 36, 37].map(n => [n, { note: 'Used by octal flash or PSRAM on ESP32-S3R8 modules; avoid on those.', kind: 'caution' as PinKind }])),
      48: { functions: ['RGB LED (DevKitC-1 v1.0)'], note: 'Addressable WS2812 LED on DevKitC-1 v1.0; v1.1 boards moved it to GPIO38.' } },
    pick: { led: 15, button: 16, analog: 1, sda: 8, scl: 9, tx: 17, rx: 18 } },
  { id: 'esp32', name: 'ESP32', summary: 'The original dual-core ESP32 with Wi-Fi, Bluetooth Classic and Bluetooth LE.',
    abilities: ['Wi-Fi (2.4 GHz)', 'Bluetooth Classic + LE', '18 analog inputs', '2 analog outputs (DAC)', 'Touch sensing', 'PWM on any pin', 'I²C, SPI, UART, I²S'],
    gpios: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23, 25, 26, 27, 32, 33, 34, 35, 36, 39],
    adc: { 0: 'ADC2_CH1', 2: 'ADC2_CH2', 4: 'ADC2_CH0', 12: 'ADC2_CH5', 13: 'ADC2_CH4', 14: 'ADC2_CH6', 15: 'ADC2_CH3', 25: 'ADC2_CH8', 26: 'ADC2_CH9', 27: 'ADC2_CH7', 32: 'ADC1_CH4', 33: 'ADC1_CH5', 34: 'ADC1_CH6', 35: 'ADC1_CH7', 36: 'ADC1_CH0', 39: 'ADC1_CH3' },
    strap: [0, 2, 5, 12, 15], usb: {}, flash: [6, 7, 8, 9, 10, 11], uart: { 1: 'UART0 TX', 3: 'UART0 RX' }, boot: 0,
    extra: { ...Object.fromEntries([34, 35, 36, 39].map(n => [n, { functions: ['Input only'], note: 'Input only, with no internal pull-up or pull-down.' }])),
      ...Object.fromEntries([16, 17].map(n => [n, { note: 'Used by PSRAM on WROVER modules; free on WROOM.', kind: 'caution' as PinKind }])),
      25: { functions: ['Analog out (DAC1)'] }, 26: { functions: ['Analog out (DAC2)'] },
      2: { functions: ['Onboard LED on many dev boards'] } },
    pick: { led: 23, button: 18, analog: 34, sda: 21, scl: 22, tx: 25, rx: 26 } },
];
const ADC2_WIFI = 'ADC2 pins cannot read analog values while Wi-Fi is on; use ADC1 pins for sensors.';

const esp = (s: EspSpec): Profile => ({
  id: s.id, name: s.name, summary: s.summary, logic: '3.3 V (not 5 V tolerant)', baud: 115200, ending: 'lf', layout: 'list',
  pinNote: `Numbers are the chip’s GPIO numbers; dev boards print them beside the pins (“${s.pick.led}”, “IO${s.pick.led}” or “GPIO${s.pick.led}”).${s.id === 'esp32' || s.id === 'esp32s3' ? ' ' + ADC2_WIFI : ''}`,
  abilities: s.abilities, commands: [], tips: ESP_TIPS, pins: espPins(s), recipes: espRecipes(s),
});

// ---- Flipper Zero -------------------------------------------------------------------------
const FLIPPER_PINS: Pin[] = [
  { id: '5V', label: '5V', number: 1, kind: 'power', functions: ['5 V out'], note: 'Off by default. Turn it on with `power 5v 1` or GPIO › 5V on GPIO. Up to about 1 A.' },
  { id: 'PA7', label: 'A7', number: 2, kind: 'io', functions: ['Digital in/out', 'Analog in', 'PWM', 'SPI MOSI'] },
  { id: 'PA6', label: 'A6', number: 3, kind: 'io', functions: ['Digital in/out', 'Analog in', 'SPI MISO'] },
  { id: 'PA4', label: 'A4', number: 4, kind: 'io', functions: ['Digital in/out', 'Analog in', 'PWM', 'SPI CS'] },
  { id: 'PB3', label: 'B3', number: 5, kind: 'io', functions: ['Digital in/out', 'SPI SCK'] },
  { id: 'PB2', label: 'B2', number: 6, kind: 'io', functions: ['Digital in/out'] },
  { id: 'PC3', label: 'C3', number: 7, kind: 'io', functions: ['Digital in/out', 'Analog in'] },
  { id: 'GND8', label: 'GND', number: 8, kind: 'ground', functions: ['Ground'] },
  { id: '3V3', label: '3V3', number: 9, kind: 'power', functions: ['3.3 V out'], note: 'Always on while the Flipper is awake. Keep the load under about 1.2 A.' },
  { id: 'PA14', label: 'SWC', number: 10, kind: 'reserved', functions: ['SWD clock (debug)'], note: 'Debug port: for flashing or debugging another chip with a probe app.' },
  { id: 'GND11', label: 'GND', number: 11, kind: 'ground', functions: ['Ground'] },
  { id: 'PA13', label: 'SIO', number: 12, kind: 'reserved', functions: ['SWD data (debug)'], note: 'Debug port: for flashing or debugging another chip with a probe app.' },
  { id: 'PB6', label: 'TX', number: 13, kind: 'caution', functions: ['UART TX', 'Digital in/out'], note: 'The Flipper’s serial transmit line: connect it to the other device’s RX.' },
  { id: 'PB7', label: 'RX', number: 14, kind: 'caution', functions: ['UART RX', 'Digital in/out'], note: 'The Flipper’s serial receive line: connect it to the other device’s TX.' },
  { id: 'PC1', label: 'C1', number: 15, kind: 'io', functions: ['Digital in/out', 'Analog in', 'I²C SDA'] },
  { id: 'PC0', label: 'C0', number: 16, kind: 'io', functions: ['Digital in/out', 'Analog in', 'I²C SCL'] },
  { id: 'PB14', label: '1W', number: 17, kind: 'caution', functions: ['1-Wire / iButton', 'Digital in/out'], note: 'Shared with the iButton contacts on the back.' },
  { id: 'GND18', label: 'GND', number: 18, kind: 'ground', functions: ['Ground'] },
];
const FLIPPER: Profile = {
  id: 'flipper', name: 'Flipper Zero',
  summary: 'Pocket multi-tool for radio and hardware: Sub-GHz, NFC, 125 kHz RFID, infrared, iButton and a GPIO header, with a command line over USB.',
  logic: '3.3 V (inputs tolerate 5 V)', baud: 115200, ending: 'cr', layout: 'row',
  pinNote: 'Numbers match the labels printed on the case above the header. Pin names (PA7, PC0, …) are what the gpio command and scripts use.',
  abilities: ['Sub-GHz radio (300–928 MHz)', 'NFC read/emulate', '125 kHz RFID', 'Infrared send/learn', 'iButton / 1-Wire', 'Bluetooth LE', 'GPIO, I²C, SPI, UART', 'JavaScript apps', 'microSD storage'],
  commands: [
    { label: 'help', text: 'help', note: 'Every command this firmware has' },
    { label: 'Device info', text: 'device_info' },
    { label: 'Battery', text: 'info power' },
    { label: 'Uptime', text: 'uptime' },
    { label: 'Green LED on', text: 'led g 255', note: 'Needs the main menu' },
    { label: 'LED off', text: 'led g 0' },
    { label: 'Buzz', text: 'vibro 1', note: 'Needs the main menu' },
    { label: 'Stop buzz', text: 'vibro 0' },
    { label: 'SD card', text: 'storage list /ext' },
    { label: 'Learn IR', text: 'ir rx', note: 'Point a remote at the Flipper; Ctrl-C stops' },
    { label: 'Ctrl-C', text: '\x03', raw: true, note: 'Stops a running command' },
  ],
  pinCommands: { read: 'gpio read {pin}', high: 'gpio mode {pin} 1\rgpio set {pin} 1', low: 'gpio mode {pin} 1\rgpio set {pin} 0' },
  tips: [
    'LED, vibration, GPIO and I²C commands refuse to run while an app is open: press Back until the Flipper shows its main screen.',
    'Press Enter in the console for a prompt (>:), then type help.',
    'Closing the qFlipper app frees the port if Connect fails.',
  ],
  pins: FLIPPER_PINS,
  recipes: [
    { id: 'led', title: 'Light an LED from the console', parts: 'LED, 220–330 Ω resistor',
      wires: [{ part: 'LED long leg (+)', pin: 'PA7', via: '220 Ω resistor' }, { part: 'LED short leg (−)', pin: 'GND8' }],
      steps: ['Leave the Flipper on its main screen.', 'Try it: the first command makes A7 an output and sets it high, the second turns it off.'],
      try: ['gpio mode PA7 1\rgpio set PA7 1', 'gpio set PA7 0'], goal: 'blink an LED on pin A7 (PA7) from a Flipper JavaScript script' },
    { id: 'button', title: 'Read a button', parts: 'Push button, 10 kΩ resistor',
      wires: [{ part: 'Button, one side', pin: 'PC0' }, { part: 'Button, other side', pin: '3V3' }, { part: '10 kΩ to ground', pin: 'GND18', via: 'from C0' }],
      steps: ['The resistor holds C0 at 0 until the button connects it to 3.3 V.', 'Hold the button and read the pin.'],
      try: ['gpio mode PC0 0\rgpio read PC0'], goal: 'show on the Flipper screen whether a button on pin C0 is pressed, from a JavaScript script' },
    { id: 'i2c', title: 'Find an I²C sensor', parts: 'I²C module running at 3.3 V',
      wires: [{ part: 'VCC', pin: '3V3' }, { part: 'GND', pin: 'GND8' }, { part: 'SDA', pin: 'PC1' }, { part: 'SCL', pin: 'PC0' }],
      steps: ['C1 is SDA and C0 is SCL on the Flipper’s external bus.', 'The scan lists the address of everything that answers.'],
      try: ['i2c'], goal: 'read an I2C sensor on the Flipper’s C1 (SDA) and C0 (SCL) pins from a JavaScript script and print its values' },
    { id: 'uart', title: 'Watch another board’s serial port', parts: 'Any 3.3 V board with a UART (ESP32, Arduino at 3.3 V, router debug port)',
      wires: [{ part: 'Board TX', pin: 'PB7' }, { part: 'Board RX', pin: 'PB6' }, { part: 'Board GND', pin: 'GND11' }],
      steps: ['On the Flipper open GPIO › USB-UART Bridge and pick the baud rate.', 'A second serial port appears on this computer: select it here to read the other board.'],
      goal: 'use the Flipper as a USB-UART bridge to read another board’s console' },
  ],
};

// ---- Raspberry Pi Pico --------------------------------------------------------------------
const PICO_GP: Record<number, string[]> = {
  0: ['UART0 TX', 'I²C0 SDA', 'SPI0 RX'], 1: ['UART0 RX', 'I²C0 SCL', 'SPI0 CS'], 2: ['I²C1 SDA', 'SPI0 SCK'], 3: ['I²C1 SCL', 'SPI0 TX'],
  4: ['UART1 TX', 'I²C0 SDA (default)', 'SPI0 RX'], 5: ['UART1 RX', 'I²C0 SCL (default)', 'SPI0 CS'], 6: ['I²C1 SDA', 'SPI0 SCK'], 7: ['I²C1 SCL', 'SPI0 TX'],
  8: ['UART1 TX', 'I²C0 SDA', 'SPI1 RX'], 9: ['UART1 RX', 'I²C0 SCL', 'SPI1 CS'], 10: ['I²C1 SDA', 'SPI1 SCK'], 11: ['I²C1 SCL', 'SPI1 TX'],
  12: ['UART0 TX', 'I²C0 SDA', 'SPI1 RX'], 13: ['UART0 RX', 'I²C0 SCL', 'SPI1 CS'], 14: ['I²C1 SDA', 'SPI1 SCK'], 15: ['I²C1 SCL', 'SPI1 TX'],
  16: ['UART0 TX', 'I²C0 SDA', 'SPI0 RX (default)'], 17: ['UART0 RX', 'I²C0 SCL', 'SPI0 CS (default)'], 18: ['I²C1 SDA', 'SPI0 SCK (default)'], 19: ['I²C1 SCL', 'SPI0 TX (default)'],
  20: ['I²C0 SDA'], 21: ['I²C0 SCL'], 22: [], 26: ['Analog in (ADC0)', 'I²C1 SDA'], 27: ['Analog in (ADC1)', 'I²C1 SCL'], 28: ['Analog in (ADC2)'],
};
const gp = (n: number, number: number): Pin => ({ id: `GP${n}`, label: `GP${n}`, number, kind: 'io', functions: ['Digital in/out, PWM', ...PICO_GP[n]] });
const gnd = (number: number): Pin => ({ id: `GND${number}`, label: 'GND', number, kind: 'ground', functions: ['Ground'] });
const PICO_PINS: Pin[] = [
  gp(0, 1), gp(1, 2), gnd(3), gp(2, 4), gp(3, 5), gp(4, 6), gp(5, 7), gnd(8), gp(6, 9), gp(7, 10),
  gp(8, 11), gp(9, 12), gnd(13), gp(10, 14), gp(11, 15), gp(12, 16), gp(13, 17), gnd(18), gp(14, 19), gp(15, 20),
  gp(16, 21), gp(17, 22), gnd(23), gp(18, 24), gp(19, 25), gp(20, 26), gp(21, 27), gnd(28), gp(22, 29),
  { id: 'RUN', label: 'RUN', number: 30, kind: 'caution', functions: ['Reset'], note: 'Connect to GND briefly to reset the board.' },
  gp(26, 31), gp(27, 32), gnd(33), gp(28, 34),
  { id: 'ADC_VREF', label: 'VREF', number: 35, kind: 'reserved', functions: ['ADC reference'], note: 'Leave it alone unless you add a precision reference.' },
  { id: '3V3', label: '3V3', number: 36, kind: 'power', functions: ['3.3 V out'], note: 'Up to about 300 mA for sensors and modules.' },
  { id: '3V3_EN', label: '3V3_EN', number: 37, kind: 'caution', functions: ['3.3 V regulator enable'], note: 'Pulling it low turns the whole board off.' },
  gnd(38),
  { id: 'VSYS', label: 'VSYS', number: 39, kind: 'power', functions: ['1.8–5.5 V in'], note: 'Battery or external supply input.' },
  { id: 'VBUS', label: 'VBUS', number: 40, kind: 'power', functions: ['5 V from USB'], note: 'USB 5 V. Never connect it to a GPIO.' },
];
const PICO: Profile = {
  id: 'pico', name: 'Raspberry Pi Pico',
  summary: 'RP2040 or RP2350 microcontroller with programmable I/O (PIO); usually runs MicroPython, CircuitPython, Arduino or the Pico SDK.',
  logic: '3.3 V (not 5 V tolerant)', baud: 115200, ending: 'crlf', layout: 'dual',
  pinNote: 'Pin 1 is the top-left corner next to the USB port; numbers run down the left side and back up the right. Pico W and Pico 2 use the same layout.',
  abilities: ['Dual-core Arm', 'PIO state machines', '3 analog inputs', 'PWM on every pin', 'I²C, SPI, UART', 'USB device', 'Wi-Fi + Bluetooth on Pico W'],
  commands: [
    { label: 'Ctrl-C', text: '\x03', raw: true, note: 'Stop the running program for a >>> prompt' },
    { label: 'Soft reset', text: '\x04', raw: true, note: 'Ctrl-D: restart MicroPython' },
    { label: 'Board info', text: 'import sys; print(sys.implementation)' },
    { label: 'Toggle LED', text: 'from machine import Pin; Pin("LED", Pin.OUT).toggle()' },
    { label: 'Files', text: 'import os; print(os.listdir())' },
  ],
  tips: ['Press Ctrl-C in the console to stop main.py and reach the >>> prompt.', 'Hold BOOTSEL while plugging in to mount it as a drive for new firmware.'],
  pins: PICO_PINS,
  recipes: [
    { id: 'led', title: 'Blink an LED', parts: 'LED, 220–330 Ω resistor',
      wires: [{ part: 'LED long leg (+)', pin: 'GP15', via: '220 Ω resistor' }, { part: 'LED short leg (−)', pin: 'GND18' }],
      steps: ['GP15 and the ground next to it sit at pins 20 and 18, the bottom-left corner.'],
      try: ['from machine import Pin; Pin(15, Pin.OUT).toggle()'], goal: 'blink an LED on GP15 once a second' },
    { id: 'analog', title: 'Read a potentiometer', parts: '10 kΩ potentiometer',
      wires: [{ part: 'Outer leg', pin: '3V3' }, { part: 'Middle leg (wiper)', pin: 'GP26' }, { part: 'Other outer leg', pin: 'GND33' }],
      steps: ['GP26 is ADC0. Readings run 0–65535 in MicroPython.'],
      try: ['from machine import ADC; print("pot=%d" % ADC(26).read_u16())'], goal: 'print the ADC0 (GP26) reading ten times a second as pot=<value>' },
    { id: 'i2c', title: 'Connect an I²C sensor or display', parts: 'I²C module at 3.3 V',
      wires: [{ part: 'VCC', pin: '3V3' }, { part: 'GND', pin: 'GND8' }, { part: 'SDA', pin: 'GP4' }, { part: 'SCL', pin: 'GP5' }],
      steps: ['GP4/GP5 are the default I²C0 pins in most libraries.'],
      try: ['from machine import I2C, Pin; print([hex(a) for a in I2C(0, sda=Pin(4), scl=Pin(5)).scan()])'], goal: 'scan I2C0 on GP4 (SDA) and GP5 (SCL) and read the sensor it finds' },
  ],
};

// ---- Arduino Uno --------------------------------------------------------------------------
const d = (n: number, functions: string[], kind: PinKind = 'io', note?: string): Pin => ({ id: `D${n}`, label: `D${n}`, kind, functions: ['Digital in/out', ...functions], note });
const UNO: Profile = {
  id: 'uno', name: 'Arduino Uno / Nano',
  summary: 'ATmega328P board: simple, sturdy 5 V I/O, programmed with the Arduino IDE or PlatformIO.',
  logic: '5 V', baud: 9600, ending: 'lf', layout: 'list',
  pinNote: 'Labels match the board’s silkscreen. Other Arduino boards (Mega, Leonardo, Nano ESP32) have different pins.',
  abilities: ['14 digital pins', '6 PWM outputs', '6 analog inputs (10-bit)', 'I²C, SPI, UART', '5 V logic', 'Onboard LED on D13'],
  commands: [], tips: ['The console shows whatever the sketch prints; the baud rate must match Serial.begin(…).', 'Opening the console resets the board, so you always see the start of the sketch.'],
  pins: [
    { id: '5V', label: '5V', kind: 'power', functions: ['5 V out'], note: 'About 400 mA from USB.' },
    { id: '3V3', label: '3.3V', kind: 'power', functions: ['3.3 V out'], note: 'Only about 50 mA.' },
    { id: 'VIN', label: 'VIN', kind: 'power', functions: ['7–12 V in'] },
    { id: 'GND', label: 'GND', kind: 'ground', functions: ['Ground'] },
    { id: 'RESET', label: 'RESET', kind: 'caution', functions: ['Reset'], note: 'Pull low to reset.' },
    { id: 'AREF', label: 'AREF', kind: 'reserved', functions: ['Analog reference'] },
    d(0, ['UART RX'], 'caution', 'Shared with USB serial: anything wired here can break uploads.'),
    d(1, ['UART TX'], 'caution', 'Shared with USB serial: anything wired here can break uploads.'),
    d(2, ['Interrupt']), d(3, ['PWM', 'Interrupt']), d(4, []), d(5, ['PWM']), d(6, ['PWM']), d(7, []), d(8, []), d(9, ['PWM']),
    d(10, ['PWM', 'SPI SS']), d(11, ['PWM', 'SPI MOSI']), d(12, ['SPI MISO']), d(13, ['SPI SCK', 'Onboard LED']),
    ...[0, 1, 2, 3, 4, 5].map(n => ({ id: `A${n}`, label: `A${n}`, kind: 'io' as PinKind, functions: ['Analog in', 'Digital in/out', ...(n === 4 ? ['I²C SDA'] : n === 5 ? ['I²C SCL'] : [])] })),
  ],
  recipes: [
    { id: 'led', title: 'Blink an LED', parts: 'LED, 220 Ω resistor',
      wires: [{ part: 'LED long leg (+)', pin: 'D9', via: '220 Ω resistor' }, { part: 'LED short leg (−)', pin: 'GND' }],
      steps: ['D9 has PWM, so analogWrite can also fade it.'], goal: 'fade an LED on D9 up and down and print the brightness as level=<0-255>' },
    { id: 'analog', title: 'Read a potentiometer', parts: '10 kΩ potentiometer',
      wires: [{ part: 'Outer leg', pin: '5V' }, { part: 'Middle leg (wiper)', pin: 'A0' }, { part: 'Other outer leg', pin: 'GND' }],
      steps: ['analogRead(A0) gives 0–1023.'], goal: 'print analogRead(A0) ten times a second as pot=<value>' },
    { id: 'i2c', title: 'Connect an I²C sensor or display', parts: 'I²C module',
      wires: [{ part: 'VCC', pin: '5V' }, { part: 'GND', pin: 'GND' }, { part: 'SDA', pin: 'A4' }, { part: 'SCL', pin: 'A5' }],
      steps: ['Check the module accepts 5 V; many 3.3 V-only sensors need a level shifter.'], goal: 'scan the I2C bus (A4/A5) and print every address that answers' },
  ],
};

// ---- USB-serial adapter -------------------------------------------------------------------
const BRIDGE: Profile = {
  id: 'bridge', name: 'USB-serial adapter',
  summary: 'A bridge between USB and a UART. On its own it does nothing: it reads and talks to whatever you wire to it, or to the chip on the dev board it is soldered to.',
  logic: 'Usually 3.3 V or 5 V, set by a jumper or switch', baud: 115200, ending: 'lf', layout: 'list',
  pinNote: 'Names vary by adapter (TXD/TXO, RXD/RXI). If this bridge is built into a dev board, pick the board’s chip above instead.',
  abilities: ['Read a device’s serial console', 'Send commands to firmware', 'DTR/RTS for auto-reset', 'Flash ESP32 and Arduino chips'],
  commands: [], tips: ['Most dev boards with a CP210x or CH340 chip are ESP32 or Arduino clones: connect, press Reset and read the boot log.', 'If you see nothing, swap TX and RX; it is the most common wiring mistake.'],
  pins: [
    { id: 'TX', label: 'TX', kind: 'io', functions: ['Data out'], note: 'Goes to the target’s RX.' },
    { id: 'RX', label: 'RX', kind: 'io', functions: ['Data in'], note: 'Goes to the target’s TX.' },
    { id: 'GND', label: 'GND', kind: 'ground', functions: ['Ground'], note: 'Always connect ground, or the data is noise.' },
    { id: 'VCC', label: 'VCC', kind: 'power', functions: ['3.3 V or 5 V out'], note: 'Only to power a target that has no power of its own.' },
    { id: 'DTR', label: 'DTR', kind: 'caution', functions: ['Reset line'], note: 'Drives auto-reset on Arduino (via 100 nF) and ESP32 boards.' },
    { id: 'RTS', label: 'RTS', kind: 'caution', functions: ['Boot-mode line'], note: 'Drives IO0 on ESP32 auto-programming circuits.' },
  ],
  recipes: [
    { id: 'uart', title: 'Read a device’s debug console', parts: 'Target device with TX/RX pads (router, ESP module, sensor)',
      wires: [{ part: 'Target TX', pin: 'RX' }, { part: 'Target RX', pin: 'TX' }, { part: 'Target GND', pin: 'GND' }],
      steps: ['Set the adapter to the target’s voltage before connecting.', 'Connect here, then power the target and watch its boot log.', 'Try 115200 first, then 9600 or 57600 if the text is garbled.'],
      goal: 'work out the baud rate and protocol of the device on this serial adapter' },
  ],
};

/** Compact guides cover the labelled header pins used below, not an MCU-wide pinout. */
function modelProfile(m: typeof models[number]): Profile {
  const arduino = m.name.startsWith('Arduino ');
  const logic = m.power === '5V' ? '5 V' : '3.3 V';
  const pin = (id: string, kind: PinKind, role: string): Pin => ({ id, label: id, kind, functions: [role] });
  return {
    id: m.id, name: m.name, summary: m.summary, logic, layout: 'list',
    baud: arduino ? 9600 : 115200, ending: arduino ? 'lf' : 'crlf',
    abilities: [...m.abilities, 'Analog input', 'I²C'],
    pinNote: 'Selected header pins for these projects, not the entire board. Match the printed labels; the I²C recipe uses header pins, not a separate connector.',
    commands: arduino ? [] : [{ label: 'Board info (CircuitPython)', text: 'import board; print(board.board_id)' }],
    tips: [arduino ? 'Match the baud rate in Serial.begin(). A bootloader port may disappear when the sketch starts.' : 'On CircuitPython, press Ctrl-C to stop the running program before using the Python console.'],
    pins: [pin(m.power, 'power', `${logic} supply`), pin('GND', 'ground', 'Ground'), pin(m.analog, 'io', 'Analog input'), pin(m.sda, 'io', 'I²C data (SDA)'), pin(m.scl, 'io', 'I²C clock (SCL)')],
    recipes: [
      { id: 'analog', title: 'Read a dial', parts: '10 kΩ potentiometer',
        wires: [{ part: 'Outer leg', pin: m.power }, { part: 'Middle leg', pin: m.analog }, { part: 'Other outer leg', pin: 'GND' }],
        steps: ['Turn the dial while watching the serial readings.'], goal: `print the analog reading on ${m.analog} as dial=<value>` },
      { id: 'i2c', title: 'Find an I²C sensor', parts: `I²C breakout rated for ${logic} power and logic, with pull-ups`,
        wires: [{ part: 'Supply', pin: m.power }, { part: 'Ground', pin: 'GND' }, { part: 'Data', pin: m.sda }, { part: 'Clock', pin: m.scl }],
        steps: [`Check the breakout accepts ${logic} on both its supply and signal pins before connecting it.`], goal: `scan the header I²C bus on ${m.sda}/${m.scl} and print the addresses found` },
    ],
  };
}

const MODEL_PROFILES = models.map(modelProfile);
export const PROFILES: Profile[] = [...ESP_SPECS.map(esp), FLIPPER, PICO, UNO, ...MODEL_PROFILES, BRIDGE];
export const profile = (id: string | null | undefined) => PROFILES.find(p => p.id === id) ?? null;

/** A guess at the board, with what it is based on and which other profiles are plausible. */
export type Detection = { id: string | null; reason: string; candidates: string[] };
const ESPS = ESP_SPECS.map(s => s.id);

/** The chip an ESP boot log or app log names, e.g. `ESP-ROM:esp32c6-20220919`. */
export function chipFromLog(log: string): string | null {
  const rom = /ESP-ROM:\s*(esp32(?:c3|c6|s3)?)\b/i.exec(log) ?? /\bchip(?: type)?[: ]+(esp32(?:-?(?:c3|c6|s3))?)\b/i.exec(log);
  if (rom) return rom[1].toLowerCase().replace('-', '');
  // The original ESP32's ROM names itself only by its build date.
  if (/^ets (?:Jun {1,2}8 2016|Jul 29 2019)/m.test(log)) return 'esp32';
  return null;
}

export function detectBoard(board: Pick<Board, 'product' | 'kind' | 'usb_id'> | undefined, log = ''): Detection {
  const chip = chipFromLog(log);
  if (chip) return { id: chip, reason: 'Named in its boot log', candidates: ESPS };
  if (!board) return { id: null, reason: '', candidates: [] };
  const kind = board.kind ?? '', product = (board.product ?? '').toLowerCase();
  if (kind === 'Flipper Zero' || product.includes('flipper')) return { id: 'flipper', reason: 'From its USB name', candidates: [] };
  const model = MODEL_PROFILES.find(p => p.name === kind);
  if (model) return { id: model.id, reason: 'From its model-specific USB ID', candidates: [] };
  const normalize = (name: string) => name.toLowerCase().replace(/^(adafruit|arduino) /, '').replace(/\s+/g, '');
  const named = MODEL_PROFILES.find(p => p.name.toLowerCase() === product || (
    kind === (p.name.startsWith('Arduino ') ? 'Arduino board' : 'Adafruit board') && normalize(p.name) === normalize(product)
  ));
  if (named) return { id: named.id, reason: 'From its USB name', candidates: [] };
  for (const s of ESP_SPECS) {
    if (new RegExp(`\\b${s.name}\\b`, 'i').test(board.product ?? '')) return { id: s.id, reason: 'From its USB name', candidates: ESPS };
  }
  if (kind === 'Arduino Uno') return { id: 'uno', reason: 'From its model-specific USB ID', candidates: [] };
  if (kind === 'Arduino board' || kind === 'Adafruit board') {
    const arduino = kind === 'Arduino board';
    const candidates = MODEL_PROFILES.filter(p => p.name.startsWith(arduino ? 'Arduino ' : 'Adafruit ')).map(p => p.id);
    return { id: null, reason: 'The USB ID identifies the maker, not the model. Match the printed board name before wiring.', candidates: arduino ? ['uno', ...candidates] : candidates };
  }
  if (kind.startsWith('Raspberry Pi RP2')) {
    if (/^(?:raspberry pi )?pico(?: 2)?(?: w)?$/i.test(product)) return { id: 'pico', reason: 'From its USB name', candidates: [] };
    return { id: null, reason: 'RP-series USB ID; the header depends on the board.', candidates: ['pico', 'feather-rp2040', 'qtpy-rp2040'] };
  }
  if (kind.startsWith('Espressif')) return { id: null, reason: 'An Espressif chip with native USB. Press Reset in the console to read its name from the boot log, or choose it.', candidates: ['esp32c6', 'esp32c3', 'esp32s3'] };
  if (/bridge/.test(kind)) return { id: null, reason: 'A USB-serial chip, often on an ESP32 or Arduino clone. Press Reset in the console to read the boot log, or choose the board.', candidates: ['esp32', 'esp32s3', 'esp32c3', 'uno', 'bridge'] };
  return { id: null, reason: '', candidates: [] };
}

/** A console command for a pin: `{pin}` replaced, and multi-line commands split into lines. */
export function pinCommand(template: string, pin: Pin) {
  return template.replaceAll('{pin}', pin.id).split('\r');
}

/** The pins a recipe uses, each with the part lead it connects to. */
export function recipeRoles(recipe: Recipe | null | undefined) {
  const roles = new Map<string, string>();
  for (const w of recipe?.wires ?? []) roles.set(w.pin, roles.has(w.pin) ? `${roles.get(w.pin)}, ${w.part}` : w.part);
  return roles;
}

/** Physical header order for a two-column layout: rows of [left, right]. */
export function dualRows(pins: Pin[]) {
  const half = Math.ceil(pins.length / 2), rows: [Pin, Pin | undefined][] = [];
  for (let i = 0; i < half; i++) rows.push([pins[i], pins[pins.length - 1 - i]]);
  return rows;
}
