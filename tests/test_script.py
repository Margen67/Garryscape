#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Test Git Bash orchestration with a fake compiler, not a real MDL compile."""
import json
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
b=out/ns/'regions'/key
s=b/'source/garryscape'/ns/'regions'/key;s.mkdir(parents=True,exist_ok=True)
(s/'model.qc').write_text(f'$modelname "garryscape/{ns}/regions/{key}.mdl"\\n')
(b/'game').mkdir(exist_ok=True)
preview=b/'preview';preview.mkdir(exist_ok=True)
(b/'preview-atlas.png').write_bytes(b'atlas')
(preview/f'{key}.gltf').write_text(json.dumps({'textures':[{'sampler':0,'source':0}],'images':[{'uri':f'{key}.png'}]}))
m=b/'game/materials/models/garryscape'/ns/'regions';m.mkdir(parents=True,exist_ok=True)
(m/f'{key}_0.vmt').write_text('material');(m/f'{key}.vtf').write_text('texture')
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
if mode=='nothing':sys.exit(0)
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
        (self.repo/'tools').mkdir()
        shutil.copy(REPO/'tools/encode_preview.py', self.repo/'tools')
        shutil.copy(REPO/'LICENSE',self.repo/'LICENSE')
        shutil.copy(REPO/'rust-toolchain.toml',self.repo/'rust-toolchain.toml')
        (self.repo/'addons/garryscape/lua/autorun/server').mkdir(parents=True)
        shutil.copy(REPO/'addons/garryscape/lua/autorun/server/garryscape_preview.lua',self.repo/'addons/garryscape/lua/autorun/server')
        (self.repo/'addons/garryscape/lua/autorun/client').mkdir(parents=True)
        (self.repo/'addons/garryscape/lua/garryscape').mkdir(parents=True)
        shutil.copy(REPO/'addons/garryscape/lua/autorun/client/garryscape_utilities.lua',self.repo/'addons/garryscape/lua/autorun/client')
        shutil.copy(REPO/'addons/garryscape/lua/autorun/client/garryscape_shader.lua',self.repo/'addons/garryscape/lua/autorun/client')
        shutil.copy(REPO/'addons/garryscape/lua/garryscape/atlas_shader.lua',self.repo/'addons/garryscape/lua/garryscape')
        (self.repo/'converter/target/release').mkdir(parents=True)
        self.exe(self.repo/'converter/target/release/garryscape-convert',CLI)
        self.bin=self.path/'bin';self.bin.mkdir();self.exe(self.bin/'cargo','#!/bin/sh\ntest "$PWD" = "$GARRYSCAPE_TEST_CARGO_ROOT" || exit 41\ntest -f rust-toolchain.toml || exit 42\nexit 0\n')
        self.exe(self.bin/'oxipng', '#!/usr/bin/env python3\nimport sys\nassert sys.argv[1:3] == ["-o", "max"]\n')
        self.exe(self.bin/'cwebp', '#!/usr/bin/env python3\nimport pathlib, sys\nassert sys.argv[1:6] == ["-quiet", "-lossless", "-z", "9", "-exact"]\npathlib.Path(sys.argv[-1]).write_bytes(b"webp")\n')
        self.compiler=self.path/'fake studiomdl.exe';self.exe(self.compiler,COMPILER)
        self.cache=self.path/'cache';self.cache.mkdir();(self.cache/'main_file_cache.dat2').write_bytes(b'cache')
        self.addon=self.path/'installed addon';self.counter=self.path/'counter'
        self.env=dict(os.environ,PATH=str(self.bin)+os.pathsep+os.environ['PATH'],COUNTER=str(self.counter),GARRYSCAPE_TEST_CARGO_ROOT=str(self.repo))
    def tearDown(self):self.tmp.cleanup()
    def exe(self,p,text):p.write_text(text);p.chmod(0o755)
    def run_script(self,*extra,fail=None,default_addon=False):
        env=self.env.copy()
        if fail:env['FAIL_MODE']=fail
        args=['bash',str(self.repo/'convert.sh'),'regions','50','50','--cache',str(self.cache),'--studiomdl',str(self.compiler),'--output','relative output',*extra]
        if not default_addon:args += ['--addon',str(self.addon)]
        return subprocess.run(args,env=env,cwd=self.path,text=True,capture_output=True)
    def test_success_reuse_and_no_source_install(self):
        r=self.run_script();self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        model=self.addon/'models/garryscape/2009scape/regions/50_50.mdl'
        stale=self.addon/'old file.txt';stale.write_text('stale')
        r=self.run_script();self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        self.assertEqual(self.counter.read_text().count('compile'),1)
        self.assertTrue(model.is_file())
        self.assertFalse(stale.exists())
        self.assertFalse((self.path/'relative output/build').exists())
        self.assertTrue((self.path/'relative output/2009scape/regions/50_50/compiled.json').is_file())
        self.assertFalse(any(p.suffix in {'.qc','.dmx','.glb','.gltf','.bin','.png','.webp'} for p in self.addon.rglob('*')))
        self.assertTrue((self.addon/'lua/autorun/server/garryscape_preview.lua').is_file())
        self.assertTrue((self.addon/'lua/autorun/client/garryscape_shader.lua').is_file())
        self.assertTrue((self.addon/'lua/autorun/client/garryscape_utilities.lua').is_file())
        self.assertTrue((self.addon/'lua/garryscape/atlas_shader.lua').is_file())
    def test_default_installs_under_garrysmod_directory(self):
        game=self.path/'Steam/GarrysMod'
        self.env['TEST_GAME_ROOT']=str(game)
        self.exe(self.bin/'cygpath','''#!/usr/bin/env python3
import os,sys
path=sys.argv[2]
if sys.argv[1]=='-u' and path.startswith('C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/'):
 path=os.environ['TEST_GAME_ROOT']+'/'+path.split('/GarrysMod/',1)[1]
print(path)
''')
        r=self.run_script(default_addon=True);self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        addon=game/'garrysmod/addons/garryscape'
        self.assertTrue((addon/'models/garryscape/2009scape/regions/50_50.mdl').is_file())
        self.assertTrue((addon/'materials/models/garryscape/2009scape/regions/50_50.vtf').is_file())
        self.assertTrue((addon/'lua/autorun/server/garryscape_preview.lua').is_file())
        self.assertFalse((game/'addons/garryscape').exists())
    def test_rebuild_recompiles(self):
        self.assertEqual(self.run_script().returncode,0)
        self.assertEqual(self.run_script('--rebuild').returncode,0)
        self.assertEqual(self.counter.read_text().count('compile'),2)
    def test_stale_outputs_do_not_hide_empty_compile(self):
        self.assertEqual(self.run_script().returncode,0)
        r=self.run_script('--rebuild',fail='nothing')
        self.assertNotEqual(r.returncode,0)
        self.assertIn('did not produce',r.stderr)
    def test_failure_is_not_installed(self):
        for mode in ['exit','message']:
            with self.subTest(mode=mode):
                r=self.run_script(fail=mode);self.assertNotEqual(r.returncode,0,r.stdout)
                self.assertFalse(self.addon.exists())
                self.assertTrue((self.path/'relative output/2009scape/regions/50_50/source/garryscape/2009scape/regions/50_50/compile.log').exists())
    def test_failure_preserves_previous_installation(self):
        self.assertEqual(self.run_script().returncode,0)
        marker=self.addon/'keep.txt';marker.write_text('previous installation')
        for mode in ['exit','message','nothing']:
            r=self.run_script('--rebuild',fail=mode)
            self.assertNotEqual(r.returncode,0)
            self.assertEqual(marker.read_text(),'previous installation')
    def test_staging_failure_preserves_previous_installation(self):
        self.assertEqual(self.run_script().returncode,0)
        marker=self.addon/'keep.txt';marker.write_text('previous installation')
        (self.repo/'LICENSE').unlink()
        r=self.run_script()
        self.assertNotEqual(r.returncode,0)
        self.assertEqual(marker.read_text(),'previous installation')
        self.assertFalse(list(self.addon.parent.glob('.garryscape-install.*')))
    def test_source_destination_is_rejected(self):
        self.addon=self.repo
        r=self.run_script()
        self.assertNotEqual(r.returncode,0)
        self.assertIn('would delete source',r.stderr)
        self.assertTrue((self.repo/'convert.sh').is_file())
    def test_preview_does_not_compile_or_install(self):
        r=self.run_script('--preview-only');self.assertEqual(r.returncode,0,r.stderr)
        self.assertFalse(self.counter.exists());self.assertFalse(self.addon.exists())
    def test_preview_texture_modes(self):
        region = self.path/'relative output/2009scape/regions/50_50/preview'
        extension = 'EXT_texture_webp'
        for mode in (None, 'webp', 'both', 'oxipng'):
            args = () if mode is None else ('--preview-texture', mode)
            result = self.run_script('--preview-only', *args)
            self.assertEqual(result.returncode, 0, result.stderr)
            doc = json.loads((region/'50_50.gltf').read_text())
            png = mode != 'webp'
            webp = mode != 'oxipng'
            self.assertEqual((region/'50_50.png').is_file(), png)
            self.assertEqual((region/'50_50.webp').is_file(), webp)
            texture = doc['textures'][0]
            self.assertEqual('source' in texture, png)
            self.assertEqual(extension in doc.get('extensionsUsed', []), webp)
            self.assertEqual(extension in doc.get('extensionsRequired', []), not png)
            if webp:
                source = texture['extensions'][extension]['source']
                self.assertEqual(doc['images'][source]['uri'], '50_50.webp')

    def test_region_required(self):
        r=subprocess.run(['bash',str(self.repo/'convert.sh'),'regions','50'],capture_output=True,text=True)
        self.assertNotEqual(r.returncode,0);self.assertIn('Specify the region',r.stderr)
    def test_empty_namespace_uses_revision(self):
        r=self.run_script('--name','');self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        self.assertTrue((self.addon/'models/garryscape/530/regions/50_50.mdl').is_file())

if __name__=='__main__':unittest.main()
