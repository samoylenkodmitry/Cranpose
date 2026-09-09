import argparse
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
root = args.output
root.mkdir(parents=True, exist_ok=False)
binary = str(args.binary.resolve())
results = []
for name, mode, budget, expected_code in [('capped', 'fifo', '120', 1), ('cadence', 'fifo', '0', 0), ('headroom', 'immediate', '1', 0), ('no-headroom', 'immediate', '1000000', 1)]:
    environment = dict(os.environ, CRANPOSE_PRESENT_MODE=mode, CRANPOSE_PERF_MIN_FPS=budget,
                       CRANPOSE_PERF_MAX_P95_FRAME_MS='0', CRANPOSE_PERF_MAX_50MS_STALLS='4294967295',
                       CRANPOSE_PERF_SCENARIO='glass_lazy_scroll', CRANPOSE_PERF_DURATION_SECS='1',
                       CRANPOSE_PERF_WARMUP_SECS='1', CRANPOSE_HEADLESS='0', CRANPOSE_MEM_VALIDATE='0')
    result = subprocess.run([binary], env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=90)
    text = result.stdout.decode(errors='replace')
    (root / f'{name}.log').write_text(text)
    assert result.returncode == expected_code, (name, result.returncode, text[-3000:])
    assert 'PERF_PRESENTATION_SUMMARY' in text
    if name == 'capped':
        assert 'resolved=Fifo' in text and 'configuration_unpaced=false' in text
        assert 'cannot measure throughput' in text and 'PERF_FPS_SUMMARY' not in text
    elif name == 'no-headroom':
        assert 'PERF_PRESENTATION_CALIBRATION' in text
        assert 'cannot measure throughput' in text and 'PERF_FPS_SUMMARY' not in text
    else:
        if name == 'headroom':
            assert 'changed=true' in text and 'headroom=true' in text
        assert 'PERF_FPS_SUMMARY' in text and 'PERF_SCENARIO_COMPLETE' in text
    results.append({'case': name, 'exit': result.returncode,
                    'presentation': next(line for line in text.splitlines() if line.startswith('PERF_PRESENTATION_SUMMARY'))})
    print(results[-1], flush=True)
(root / 'results.json').write_text(json.dumps(results, indent=2))
