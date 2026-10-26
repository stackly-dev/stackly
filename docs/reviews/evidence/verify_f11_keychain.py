"""Verify the saved F11 native evidence without accessing Keychain or secret values."""
from pathlib import Path
from datetime import datetime
import json
import copy
import re

def at(text):
 text=text.replace('Z','+00:00')
 if len(text)>=5 and text[-5] in '+-' and text[-3] != ':': text=text[:-2]+':'+text[-2:]
 text=re.sub(r'(\.\d{6})\d+',r'\1',text)
 return datetime.fromisoformat(text)

def verify(c):
 r=c['cli']; events=c['securityd']; decision=c['osDecision']; pid=c['clientPid']; ref=c['reference']
 assert len(c['audit'])==1, 'must correlate exactly one audit'
 prompt=[e for e in events if 'displaying keychain prompt' in e['eventMessage'] and f'stacklyd({pid})' in e['eventMessage'] and ref in e['eventMessage']]
 approvals=[e for e in events if "user approved 'always allow'" in e['eventMessage'] and f'stacklyd({pid})' in e['eventMessage']]
 rejects=[e for e in events if 'ObjectAcl REJECTS' in e['eventMessage'] and ref in e['eventMessage']]
 if decision=='always_allow':
  assert len(prompt)==1 and len(approvals)==1
  assert at(prompt[0]['timestamp'])<at(approvals[0]['timestamp'])<at(c['audit'][0]['at'])<=at(r['finishedAt'])
  assert r['exitCode']==0 and r['valueReturned'] and r['errorCode'] is None and c['audit'][0]['result']=='succeeded'
 else:
  assert not approvals and len(rejects)==1
  assert r['exitCode']==4 and r['errorCode']=='secret.store_access_denied' and r['valueReturned'] is False
  assert c['audit'][0]['result']=='failed' and c['audit'][0]['details']['errorCode']==r['errorCode']
  interactive=decision=='denied'
  assert len(prompt)==int(interactive)
  if interactive:
   denied=[e for e in events if "user did not approve 'allow'" in e['eventMessage'] and f'stacklyd({pid})' in e['eventMessage']]
   assert len(denied)==1 and at(prompt[0]['timestamp'])<at(denied[0]['timestamp'])<=at(rejects[0]['timestamp'])
  assert at(rejects[0]['timestamp'])<=at(c['audit'][0]['at'])<=at(r['finishedAt'])
  assert dict((d['key'],d['value']) for d in r['details'])['byUser']==str(interactive).lower()

if __name__ == '__main__':
    payload = json.loads(Path(__file__).with_name('2026-10-26-f11-keychain-correlation.json').read_text())
    for case in payload['cases']:
        verify(case)
        print('PASS', case['label'])
    negative = copy.deepcopy(payload['cases'][3])
    negative['cli']['exitCode'] = 0
    negative['cli']['valueReturned'] = True
    try:
        verify(negative)
    except AssertionError:
        print('PASS negative control: denied read returning a value rejected')
    else:
        raise RuntimeError('denied read returning a value was incorrectly accepted')
