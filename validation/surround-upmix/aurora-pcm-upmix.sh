#!/bin/sh
set -eu

FFMPEG="${AURORA_FFMPEG_BIN:-/usr/bin/ffmpeg}"
[ -x "$FFMPEG" ] || { echo "pcm-upmix: FFmpeg unavailable" >&2; exit 1; }
[ "$#" -eq 2 ] || { echo "usage: aurora-pcm-upmix <5.1|7.1> <7.1.4|11.1.4>" >&2; exit 2; }

SOURCE="$1"
TARGET="$2"

case "$SOURCE:$TARGET" in
  5.1:7.1.4)
    INPUT_LAYOUT='5.1(side)'
    INPUT_CHANNELS=6
    CHANNELS=12
    GRAPH='[0:a:0]pan=12c|c0=FL|c1=FR|c2=FC|c3=LFE|c4=0.18*SL-0.18*SR|c5=0.18*SR-0.18*SL|c6=SL|c7=SR|c8=0.12*FL-0.12*FR|c9=0.12*FR-0.12*FL|c10=0.12*SL-0.12*SR|c11=0.12*SR-0.12*SL[out]'
    ;;
  5.1:11.1.4)
    INPUT_LAYOUT='5.1(side)'
    INPUT_CHANNELS=6
    CHANNELS=16
    GRAPH='[0:a:0]pan=16c|c0=FL|c1=FR|c2=FC|c3=0.12*FL-0.12*FR|c4=0.12*FR-0.12*FL|c5=SL|c6=SR|c7=0.12*SL-0.12*SR|c8=0.12*SR-0.12*SL|c9=0.18*SL-0.18*SR|c10=0.18*SR-0.18*SL|c11=LFE|c12=0.12*FL-0.12*FR|c13=0.12*FR-0.12*FL|c14=0.12*SL-0.12*SR|c15=0.12*SR-0.12*SL[out]'
    ;;
  7.1:11.1.4)
    INPUT_LAYOUT='7.1'
    INPUT_CHANNELS=8
    CHANNELS=16
    GRAPH='[0:a:0]pan=16c|c0=FL|c1=FR|c2=FC|c3=0.12*FL-0.12*FR|c4=0.12*FR-0.12*FL|c5=SL|c6=SR|c7=0.08*SL-0.08*SR+0.08*BL-0.08*BR|c8=0.08*SR-0.08*SL+0.08*BR-0.08*BL|c9=BL|c10=BR|c11=LFE|c12=0.12*FL-0.12*FR|c13=0.12*FR-0.12*FL|c14=0.08*SL-0.08*SR+0.08*BL-0.08*BR|c15=0.08*SR-0.08*SL+0.08*BR-0.08*BL[out]'
    ;;
  *)
    echo "pcm-upmix: unsupported lane $SOURCE -> $TARGET" >&2
    exit 2
    ;;
esac

echo "aurora: mode=pcm-channel-upmix source=$SOURCE target=$TARGET source_channels=$INPUT_CHANNELS output_channels=$CHANNELS objects_decoded=false synthetic_channels=true" >&2

exec "$FFMPEG" -hide_banner -loglevel error -nostdin -threads 1 \
  -f f32le -ar 48000 -ac "$INPUT_CHANNELS" -channel_layout "$INPUT_LAYOUT" -i pipe:0 \
  -filter_complex_threads 1 -filter_complex "$GRAPH" -map '[out]' -ar 48000 \
  -c:a pcm_f32le -f f32le pipe:1
