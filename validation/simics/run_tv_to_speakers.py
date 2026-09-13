"""Fresh, staged TV-audio-boundary -> real Aurora media -> Simics speaker run.

Never claims Netflix service or physical acceptance from the fixture lane.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'validation/virtual-hardware'))
import run_aurora_sim_windows as existing


def main():
    run_id = str(uuid.uuid4())
    out = ROOT / 'artifacts/tv-to-speakers' / run_id
    out.mkdir(parents=True)
    cache = ROOT / '.cache/aurora-sim-windows'
    project = Path(r'C:\Users\ammar\simics-projects\aurora')
    env = os.environ.copy()
    phases = []

    def run(name, args, timeout=180, expected=0):
        print(name, flush=True)
        started = time.monotonic()
        with (out / (name + '.log')).open('w', encoding='utf-8') as log:
            result = subprocess.run([str(a) for a in args], cwd=ROOT, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
        phases.append({'phase': name, 'exit_code': result.returncode,
                       'expected_exit_code': expected, 'wall_seconds': time.monotonic() - started})
        if result.returncode != expected:
            raise RuntimeError(name + ' failed; see ' + str(out / (name + '.log')))

    def read(name):
        return json.loads((out / name).read_text(encoding='utf-8'))

    report = {'run_id': run_id, 'simulation_verdict': 'running',
              'netflix_real_audio': 'not_tested_no_hardware_or_service_capture',
              'physical_speakers': 'not_tested_user_has_no_devices',
              'truth_boundary': 'Staged file-coupled functional simulation. TV audio boundary and electrical speaker loads are abstract. Real host decoder/render/DSP execute freshly. Not a Netflix app, TV OS, DRM, live guest driver or simultaneous clock-coupled end-to-end system.',
              'phases': phases}
    try:
        # Begin at the official encoded source, not an existing PCM rendering.
        archive = ROOT / 'artifacts/aurora-sim-windows/work/audio.zip'
        if not archive.exists():
            archive = out / 'audio.zip'
            existing.download_verified(existing.DOLBY_URL, archive, existing.ZIP_SHA, 'Dolby fixture')
        existing.assert_sha(archive, existing.ZIP_SHA, 'official Dolby archive')
        with zipfile.ZipFile(archive) as zf:
            names = [n for n in zf.namelist() if Path(n).name == existing.DOLBY_NAME]
            assert len(names) == 1
            source = zf.read(names[0])
        assert hashlib.sha256(source).hexdigest() == existing.SOURCE_SHA
        derived = out / 'official-after-first-au.ec3'
        derived.write_bytes(source[existing.FIRST_AU_BYTES:])
        existing.assert_sha(derived, existing.DERIVED_SHA, 'exact known-malformed-AU0 suffix')
        report['source'] = {'url': existing.DOLBY_URL, 'sha256': existing.SOURCE_SHA,
                            'derived_sha256': existing.DERIVED_SHA,
                            'derivation': 'Remove exactly the known malformed first 2560-byte AU; no re-encoding.',
                            'is_netflix': False}
        run('01-wrap-tv-carrier', [shutil.which('ffmpeg'), '-nostdin', '-hide_banner', '-loglevel', 'error',
            '-y', '-i', derived, '-c:a', 'copy', '-f', 'spdif', out / 'tv-source.spdif'])
        env['AURORA_TV_OUT'] = str(out)
        env['AURORA_TV_CARRIER'] = str(out / 'tv-source.spdif')
        run('02-simics-tv-receiver', [project / 'simics.bat', '--batch-mode', ROOT / 'validation/simics/tv.simics'])
        report['tv_ingress'] = read('tv-ingress.json')
        assert report['tv_ingress']['verdict'] == 'pass'

        orender = cache / 'omniphony-target/release/orender.exe'
        bridge = cache / 'harletty-bridge-v0.7.4-windows/harletty_bridge.dll'
        telemetry = cache / 'windows-telemetry-target/release/aurora-moving-joc-telemetry-windows.exe'
        dsp = cache / 'aurora-output-dsp-target/release/examples/process_7_1_4_file.exe'
        layout = cache / 'Omniphony/layouts/7.1.4.yaml'
        for file in (orender, bridge, telemetry, dsp, layout):
            if not file.is_file():
                raise RuntimeError('Build the native Windows AuroraSim dependencies first: ' + str(file))
        report['executed_binary_sha256'] = {str(f): existing.sha256(f) for f in (orender, bridge, telemetry, dsp, layout)}
        report['binary_provenance'] = 'Reuse existing Windows AuroraSim build products; identities recorded. This run regenerates all media and telemetry, not a clean rebuild of external dependencies.'
        received = out / 'received.spdif'
        run('03-real-joc-telemetry', [telemetry, bridge, received, out / 'telemetry.json'])
        common = [orender, received, '--bridge-path', bridge, '--enable-vbap', '--speaker-layout', layout,
                  '--output-backend', 'file', '--output-file-format', 'raw-f32']
        run('04-real-7.1.4-render', common + ['--output-file', out / 'unpaced.f32'])
        run('05-real-paced-render', [sys.executable, ROOT / 'validation/virtual-hardware/pace_orender.py',
            '--orender', orender, '--bridge', bridge, '--layout', layout, '--carrier', received,
            '--unpaced-render', out / 'unpaced.f32', '--paced-render', out / 'paced.f32',
            '--log', out / 'paced-render.log', '--report', out / 'pacing.json'])
        run('06-joc-evidence', [sys.executable, ROOT / 'validation/immersive/aurora_joc_moving_evidence.py',
            'analyze', '--input', derived, '--expected-sha256', existing.DERIVED_SHA,
            '--provenance', 'Official Dolby fixture through Simics TV boundary, not Netflix or physical eARC',
            '--telemetry', out / 'telemetry.json', '--pcm', out / 'unpaced.f32', '--pacing', out / 'pacing.json',
            '--sample-rate', '48000', '--channels', '12', '--output', out / 'joc-evidence.json'])
        run('07-real-aurora-dsp', [dsp, out / 'paced.f32', out / 'dsp.f32', '0'])
        assert (out / 'dsp.f32').stat().st_size == (out / 'paced.f32').stat().st_size
        assert existing.sha256(out / 'dsp.f32') != existing.sha256(out / 'paced.f32')
        analyzer = ROOT / 'validation/virtual-hardware/aurora_full_system_sim.py'
        for fault in ('none',) + existing.FAULT_PROFILES:
            run('08-virtual-' + fault, [sys.executable, analyzer, 'run', '--render', out / 'dsp.f32',
                '--joc-evidence', out / 'joc-evidence.json', '--report', out / ('virtual-' + fault + '.json'),
                '--fault', fault, '--tdm-slots', '16', '--latency-frames', '256', '--max-latency-frames', '1024'],
                expected=0 if fault == 'none' else 1)
            fault_report = read('virtual-' + fault + '.json')
            assert fault_report['verdict'] == ('pass' if fault == 'none' else 'fail')
            if fault != 'none':
                assert fault_report['failures']
        env['AURORA_SIMICS_ROOT'] = str(ROOT)
        env['AURORA_SIMICS_PCM'] = str(out / 'dsp.f32')
        env['AURORA_SIMICS_OUT'] = str(out / 'simics-output.json')
        env['AURORA_SIMICS_RUN_ID'] = run_id
        run('09-simics-dma-to-speakers', [project / 'simics.bat', '--batch-mode', ROOT / 'validation/simics/aurora.simics'])
        output = read('simics-output.json')
        assert output['verdict'] == 'pass' and output['run_id'] == run_id
        report['simics_output'] = output
        report['pacing'] = read('pacing.json')
        assert report['pacing']['status'] == 'pass'
        report['simulation_verdict'] = 'pass'
        print('AURORA-TV-TO-SPEAKERS-SIMULATION-PASS', flush=True)
    except Exception as exc:
        report['simulation_verdict'] = 'fail'
        report['error'] = str(exc)
        raise
    finally:
        report['source_sha256'] = {p.name: existing.sha256(p) for p in Path(__file__).parent.glob('*.py')}
        (out / 'tv-to-speakers.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('REPORT=' + str(out / 'tv-to-speakers.json'), flush=True)


if __name__ == '__main__':
    main()
