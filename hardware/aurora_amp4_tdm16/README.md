# Aurora AMP4 synchronized 16-channel backend

Derivative of freeDSPx AMP x4 (CC BY-SA 4.0).

Use four boards on one 16-wire IDC bus:

- AMP4-A + JP1 closed = channels 0-3
- AMP4-A + JP1 open   = channels 4-7
- AMP4-B + JP1 closed = channels 8-11
- AMP4-B + JP1 open   = channels 12-15

All boards share MCLK/BCLK/LRCLK, so all 16 outputs are in one hardware clock domain.

AMP4-A keeps the original serial-data input on P2 pin 5.
AMP4-B reroutes only the serial-data input to previously unused P2 pin 11.
The TAS5720M power stages and their output filters are unchanged.

Before fabrication: run KiCad DRC/ERC and inspect the AMP4-B reroute. First bring-up must use a current-limited supply and dummy loads.

License: CC BY-SA 4.0. Original design attribution: freeDSP/freeDSPx-AMP-x4.
