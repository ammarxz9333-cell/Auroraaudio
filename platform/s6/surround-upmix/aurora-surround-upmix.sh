#!/bin/sh
set -eu

# External channel decoder + experimental ambience upmixer. No JOC/OAMD
# reconstruction: stderr identifies this mode even if input contains Atmos.
FFMPEG="${AURORA_FFMPEG_BIN:-/usr/bin/ffmpeg}"
[ -x "$FFMPEG" ] || { echo "surround-upmix: FFmpeg unavailable" >&2; exit 1; }
[ "$#" -eq 0 ] || { echo "usage: aurora-surround-upmix < IEC61937 > raw-f32" >&2; exit 2; }

echo "aurora: decode_mode=surround-upmix objects_decoded=false heights=synthetic" >&2

# FFmpeg's 7.1 bed order matches Aurora USB v1: FL FR FC LFE BL BR SL SR.
# Preserve that bed; derive quiet height ambience from left/right differences.
# Center dialogue and LFE have no direct route into the height bus. Band limits
# avoid sending bass to small height drivers. Unequal delays decorrelate heights.
# These are authored effects, not recovered original object positions.
GRAPH='[0:a:0]aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=7.1,asplit=2[bed][amb];[amb]pan=quad|FL=0.25*FL-0.25*FR|FR=0.25*FR-0.25*FL|BL=0.25*SL-0.25*SR+0.25*BL-0.25*BR|BR=0.25*SR-0.25*SL+0.25*BR-0.25*BL,highpass=f=250,lowpass=f=7000,allpass=f=1500,adelay=11|17|23|29[height];[bed][height]join=inputs=2:channel_layout=7.1.4:map=0.FL-FL|0.FR-FR|0.FC-FC|0.LFE-LFE|0.BL-BL|0.BR-BR|0.SL-SL|0.SR-SR|1.FL-TFL|1.FR-TFR|1.BL-TBL|1.BR-TBR[out]'

# exec keeps the broker's signal/restart ownership on one process. No shell
# pipeline, orphaned decoder child, network input or DRM extraction is involved.
exec "$FFMPEG" -hide_banner -loglevel warning -nostdin \
    -threads 1 -probesize 32768 -analyzeduration 100000 \
    -f spdif -i pipe:0 -filter_complex_threads 1 \
    -filter_complex "$GRAPH" -map '[out]' -ar 48000 \
    -c:a pcm_f32le -flush_packets 1 -f f32le pipe:1
