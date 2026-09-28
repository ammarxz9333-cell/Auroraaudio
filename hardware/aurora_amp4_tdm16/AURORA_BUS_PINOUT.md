# Aurora IDC bus

| Pin | Signal |
|---:|---|
| 1 | spare |
| 2 | GND |
| 3 | spare |
| 4 | GND |
| 5 | DATA_A (TDM8) |
| 6 | GND |
| 7 | BCLK |
| 8 | GND |
| 9 | LRCLK |
| 10 | GND |
| 11 | DATA_B (TDM8) |
| 12 | GND |
| 13 | spare |
| 14 | GND |
| 15 | MCLK |
| 16 | GND |

At 48 kHz with eight 32-bit TDM slots per data line, BCLK is 12.288 MHz.
DATA_A and DATA_B must be synchronous to the same BCLK/LRCLK/MCLK.
