#!/usr/bin/env python3
"""Prepare pinned upstream stereo encoder inputs; never patch encoded bitstreams."""
import argparse
import hashlib
import json
import pathlib
import wave


def prepare(upstream, output, config):
    source = config['encoder_inputs']
    output.mkdir(parents=True, exist_ok=True)
    hashes = {}
    for relative, expected in source['sha256'].items():
        actual = hashlib.sha256((upstream / relative).read_bytes()).hexdigest()
        if actual != expected:
            raise SystemExit(f'Pinned encoder input hash mismatch: {relative}')
        hashes[relative] = actual
    wav = upstream / source['wav']
    with wave.open(str(wav)) as stream:
        if (stream.getnchannels(), stream.getframerate(), stream.getnframes()) != (2, 48000, 24000):
            raise SystemExit('Expected upstream stereo 48 kHz, 24000-frame source')
    generated = {}
    for name, relative in source['templates'].items():
        template = (upstream / relative).read_text(encoding='utf-8')
        if template.count('file_name_prefix: "TEMPLATE"') != 1 or template.count('wav_filename: "TEMPLATE_stereo.wav"') != 1:
            raise SystemExit(f'Unexpected template substitution contract: {relative}')
        template = template.replace('file_name_prefix: "TEMPLATE"', f'file_name_prefix: "{name}"')
        template = template.replace('wav_filename: "TEMPLATE_stereo.wav"', f'wav_filename: "{wav.name}"')
        template += '\nencoder_control_metadata { add_build_information_tag: false }\n'
        path = output / f'{name}.textproto'
        path.write_text(template, encoding='utf-8', newline='\n')
        generated[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
    report = {'upstream_commit': config['pinned_commit'], 'input_sha256': hashes,
              'generated_metadata_sha256': generated, 'source_frames': 24000,
              'sample_rate': 48000, 'channels': 2,
              'provenance': 'Pinned upstream templates and WAV; only filenames and build-information tag configured. Encoded by pinned encoder_main.'}
    (output / 'encoder-inputs.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('upstream', type=pathlib.Path)
    parser.add_argument('output', type=pathlib.Path)
    parser.add_argument('config', type=pathlib.Path)
    args = parser.parse_args()
    prepare(args.upstream, args.output, json.loads(args.config.read_text(encoding='utf-8')))
