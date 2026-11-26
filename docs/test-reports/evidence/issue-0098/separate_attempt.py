import sys, time
t0 = time.time()
from spleeter.separator import Separator
sep = Separator('spleeter:2stems')
print(f'init (incl. model download): {time.time()-t0:.1f}s', flush=True)
t1 = time.time()
sep.separate_to_file('/tmp/sp-test/input.wav', '/tmp/sp-test/out')
print(f'separate: {time.time()-t1:.1f}s')
