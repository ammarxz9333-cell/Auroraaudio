#!/usr/bin/env bash
set -euo pipefail
rm -rf daviewer
git clone --depth 1 https://github.com/redtidev1918/DAViewer.git daviewer
mkdir -p daviewer/lib/features/multisource
cp -R daviewer_x_patch/lib/features/multisource/models daviewer/lib/features/multisource/
cp -R daviewer_x_patch/lib/features/multisource/sources daviewer/lib/features/multisource/
cp -R daviewer_x_patch/lib/features/multisource/discovery daviewer/lib/features/multisource/
mkdir -p daviewer/lib/features/multisource/ui
cat daviewer_x_parts/discover3_00.part daviewer_x_parts/discover3_01a.part daviewer_x_parts/discover3_01b2.part daviewer_x_parts/discover3_02.part daviewer_x_parts/discover3_03.part daviewer_x_parts/x41.txt daviewer_x_parts/x42.txt daviewer_x_parts/x43.txt > daviewer/lib/features/multisource/ui/multisource_discover_screen.dart
cat daviewer_x_parts/detail_00.part daviewer_x_parts/detail_01.part daviewer_x_parts/detail_02.part daviewer_x_parts/detail_03a.part daviewer_x_parts/detail_03b.part > daviewer/lib/features/multisource/ui/external_artwork_detail_screen.dart
python3 daviewer_x_build/integrate.py
