#!/usr/bin/env python3
"""Test Git Bash orchestration with a fake compiler, not a real MDL compile."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
CLI = '''#!/usr/bin/env python3
import json, pathlib, sys
args=sys.argv[1:]; out=pathlib.Path(args[args.index('--output')+1]); ns=args[args.index('--name')+1]; key=f'{args[1]}_{args[2]}'
b=out/'build'/ns/'regions'/key
s=b/'source/garryscape'/ns/'regions'/key;s.mkdir(parents=True,exist_ok=True)
(s/'model.qc').write_text(f'$modelname "garryscape/{ns}/regions/{key}.mdl"\\n')
(b/'game').mkdir(exist_ok=True)
m=b/'game/materials/models/garryscape'/ns/'regions';m.mkdir(parents=True,exist_ok=True)
(m/f'{key}.vmt').write_text('material');(m/f'{key}.vtf').write_text('texture')
(b/'manifest.json').write_text(json.dumps({'model':f'models/garryscape/{ns}/regions/{key}.mdl'}))
(b/'complete.json').write_text('fingerprint')
'''
COMPILER = '''#!/usr/bin/env python3
import os,pathlib,re,sys
assert os.environ['MSYS2_ARG_CONV_EXCL']=='*'
args=sys.argv[1:]; assert args[0]=='-game';game=pathlib.Path(args[1]); qc=pathlib.Path(args[2]);assert qc.is_absolute() and qc.is_file()
with open(os.environ['COUNTER'],'a') as f:f.write('compile\\n')
mode=os.environ.get('FAIL_MODE','')
if mode=='exit':sys.exit(3)
if mode=='message':print('ERROR: bad input');sys.exit(0)
model=re.search(r'"([^" ]+\\.mdl)"',qc.read_text()).group(1)
p=game/'models'/model;p.parent.mkdir(parents=True,exist_ok=True)
for suffix in ['mdl','vvd','dx90.vtx']:p.with_suffix('.'+suffix).write_text('compiled')
'''

class Script(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='garryscape script ')
        self.path = Path(self.tmp.name)
        self.repo = self.path/'repo with spaces';self.repo.mkdir()
        shutil.copy(REPO/'convert.sh',self.repo/'convert.sh')
        (self.repo/'addons/garryscape/lua/autorun/server').mkdir(parents=True)
        shutil.copy(REPO/'addons/garryscape/lua/autorun/server/garryscape_preview.lua',self.repo/'addons/garryscape/lua/autorun/server')
        (self.repo/'converter/target/release').mkdir(parents=True)
        self.exe(self.repo/'converter/target/release/garryscape-convert',CLI)
        self.bin=self.path/'bin';self.bin.mkdir();self.exe(self.bin/'cargo','#!/bin/sh\nexit 0\n')
        self.compiler=self.path/'fake studiomdl.exe';self.exe(self.compiler,COMPILER)
        self.cache=self.path/'cache';self.cache.mkdir();(self.cache/'main_file_cache.dat2').write_bytes(b'cache')
        self.addon=self.path/'installed addon';self.counter=self.path/'counter'
        self.env=dict(os.environ,PATH=str(self.bin)+os.pathsep+os.environ['PATH'],COUNTER=str(self.counter))
    def tearDown(self):self.tmp.cleanup()
    def exe(self,p,text):p.write_text(text);p.chmod(0o755)
    def run_script(self,*extra,fail=None):
        env=self.env.copy()
        if fail:env['FAIL_MODE']=fail
        return subprocess.run(['bash',str(self.repo/'convert.sh'),'regions','50','50','--cache',str(self.cache),'--studiomdl',str(self.compiler),'--addon',str(self.addon),'--output','relative output',*extra],env=env,cwd=self.path,text=True,capture_output=True)
    def test_success_reuse_and_no_source_install(self):
        r=self.run_script();self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        model=self.addon/'models/garryscape/2009scape/regions/50_50.mdl';stamp=model.stat().st_mtime_ns
        r=self.run_script();self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        self.assertEqual(self.counter.read_text().count('compile'),1)
        self.assertEqual(model.stat().st_mtime_ns,stamp)
        self.assertFalse(any(p.suffix in {'.qc','.dmx','.glb'} for p in self.addon.rglob('*')))
        self.assertTrue((self.addon/'lua/autorun/server/garryscape_preview.lua').is_file())
    def test_failure_is_not_installed(self):
        for mode in ['exit','message']:
            with self.subTest(mode=mode):
                r=self.run_script(fail=mode);self.assertNotEqual(r.returncode,0,r.stdout)
                self.assertFalse(self.addon.exists())
                self.assertTrue((self.path/'relative output/build/2009scape/regions/50_50/source/garryscape/2009scape/regions/50_50/compile.log').exists())
    def test_preview_does_not_compile_or_install(self):
        r=self.run_script('--preview-only');self.assertEqual(r.returncode,0,r.stderr)
        self.assertFalse(self.counter.exists());self.assertFalse(self.addon.exists())
    def test_region_required(self):
        r=subprocess.run(['bash',str(self.repo/'convert.sh'),'regions','50'],capture_output=True,text=True)
        self.assertNotEqual(r.returncode,0);self.assertIn('Specify the region',r.stderr)
    def test_empty_namespace_uses_revision(self):
        r=self.run_script('--name','');self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        self.assertTrue((self.addon/'models/garryscape/530/regions/50_50.mdl').is_file())

if __name__=='__main__':unittest.main()
