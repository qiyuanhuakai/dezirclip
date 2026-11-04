import array
import math
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


expect_cue = runpy.run_path(str(Path(__file__).with_name('mygo-desktop-x11.py')))['expect_cue']


class PasteSoundEvidence(unittest.TestCase):
    def test_old_complete_cue_cannot_supply_the_missing_second_note(self):
        def note(hz):
            return array.array('h', (int(5000*math.sin(i*2*math.pi*hz/22050)) for i in range(900)))

        first = note(950)
        complete = first + array.array('h', [0])*1450 + note(1100) + array.array('h', [0])*1000
        old = array.array('h', [0])*10000 + complete + array.array('h', [0])*30000
        with tempfile.TemporaryDirectory() as directory:
            audio = Path(directory)/'monitor.pcm'
            audio.write_bytes(old.tobytes())
            expect_cue(audio, 0, paste=True)
            boundary = audio.stat().st_size
            with audio.open('ab') as stream:
                (first + array.array('h', [0])*4000).tofile(stream)
            with patch('time.monotonic', side_effect=[0, 1, 2, 3, 6]), patch('time.sleep'):
                with self.assertRaisesRegex(AssertionError, 'Missing paste cue'):
                    expect_cue(audio, boundary, paste=True)


if __name__ == '__main__':
    unittest.main()
