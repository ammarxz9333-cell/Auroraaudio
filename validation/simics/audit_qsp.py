"""Inventory real instantiated QSP objects; never infer Netflix success."""
import json
import os
from pathlib import Path
import simics

objects = [{'name': obj.name, 'class': obj.classname} for obj in simics.SIM_get_all_objects()]
matches = [obj for obj in objects if any(term in (obj['name'] + ' ' + obj['class']).lower()
           for term in ('audio', 'sound', 'ac97', 'hda', 'hdmi', 'earc'))]
report = {'qsp_instantiated': True, 'object_count': len(objects),
          'audio_name_matches': matches, 'objects': objects,
          'netflix_playback': 'not_tested', 'netflix_joc_into_aurora': 'not_proven',
          'limit': 'Object inventory is a platform audit, not a guest OS boot or an exhaustive interface capability proof.'}
target = Path(os.environ['AURORA_QSP_AUDIT_OUT'])
target.write_text(json.dumps(report, indent=2), encoding='utf-8')
print('AURORA-QSP-AUDIT objects=%d audio_name_matches=%d' % (len(objects), len(matches)))
simics.SIM_quit(0)
