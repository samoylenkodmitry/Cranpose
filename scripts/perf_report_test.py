import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


class PerfReportContracts(unittest.TestCase):
    def test_success_and_failure_keep_presentation_evidence_and_exit_status(self):
        repository = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'scripts').mkdir()
            for name in ['perf_robot_fps.sh', 'scripts/dev_build_common.sh', 'scripts/perf_robot_common.sh']:
                shutil.copy2(repository / name, root / name)
            runner = root / 'cargo-dev.sh'
            metadata = shlex.quote(json.dumps({'target_directory': str(root / 'configured artifacts')}))
            runner.write_text('#!/bin/sh\nif [ "$1" = metadata ]; then printf \'%s\\n\' ' + metadata + '; fi\n')
            runner.chmod(0o755)
            binary = root / 'configured artifacts/ci/examples/robot_perf_harness'
            binary.parent.mkdir(parents=True)
            binary.write_text('''#!/bin/sh
printf '%s\\n' 'PERF_PRESENTATION_SUMMARY configuration_unpaced=true'
printf '%s\\n' 'PERF_PRESENTATION_CALIBRATION headroom=false'
exit "$FIXTURE_EXIT"
''')
            binary.chmod(0o755)
            for status in [0, 7]:
                with self.subTest(status=status):
                    report = root / f'report-{status}.txt'
                    result = subprocess.run(['bash', str(root / 'perf_robot_fps.sh'), '--profile', 'ci',
                                             '--scenario', 'glass_lazy_scroll', '--output', str(report)],
                                            cwd=root, env=dict(os.environ, CI='1', FIXTURE_EXIT=str(status),
                                                               CRANPOSE_USE_SCCACHE='0'),
                                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)
                    self.assertEqual(result.returncode, status, result.stdout.decode())
                    text = report.read_text()
                    self.assertIn('PERF_PRESENTATION_SUMMARY configuration_unpaced=true', text)
                    self.assertIn('PERF_PRESENTATION_CALIBRATION headroom=false', text)
                    self.assertIn('app_log=', text)


if __name__ == '__main__':
    unittest.main()
