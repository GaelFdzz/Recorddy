"""Prepara el auxiliar portable con fuentes verificadas; requiere MSVC solo al compilar."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORK = ROOT / 'target/rife-stream-build'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--profile-dir', type=pathlib.Path, default=ROOT/'target/fast')
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    lock = ROOT/'native/rife-stream/sources.lock.json'
    WORK.mkdir(parents=True, exist_ok=True)
    for source in json.loads(lock.read_text())['archives']:
        name, digest = source['name'], source['sha256']
        archive = WORK/(name + '.tar.gz')
        if not archive.exists():
            if args.offline: raise RuntimeError('Falta el archivo de fuentes: ' + str(archive))
            print('Descargando fuentes: ' + name, flush=True)
            request = urllib.request.Request(source['url'], headers={'User-Agent':'Recorddy-build'})
            with urllib.request.urlopen(request, timeout=90) as response, archive.open('wb') as output:
                shutil.copyfileobj(response, output)
        with archive.open('rb') as cached:
            actual = hashlib.file_digest(cached, 'sha256').hexdigest()
        if actual != digest:
            raise RuntimeError('SHA-256 incorrecto: ' + str(archive))
        folder = WORK/name
        marker = folder/'.recorddy-source-sha256'
        if marker.exists() and marker.read_text() == digest: continue
        folder.mkdir(exist_ok=True)
        with tarfile.open(archive) as tar:
            for member in tar:
                parts = pathlib.PurePosixPath(member.name).parts[1:]
                if not parts: continue
                # El archivo RIFE incluye todos los modelos; solo se compilan sus fuentes.
                if name == 'rife' and parts[0] not in ('src', 'LICENSE'): continue
                member.name = str(pathlib.PurePosixPath(*parts))
                tar.extract(member, folder, filter='data')
        marker.write_text(digest)
    build()
    dest = args.profile_dir.resolve()/'tools/rife'
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copy2(WORK/'build/recorddy-rife-stream.exe', dest)
    shutil.copy2(lock, dest/'recorddy-rife-stream-sources.json')
    for component, filename in [('rife','LICENSE'), ('ncnn','LICENSE.txt'), ('glslang','LICENSE.txt'), ('vulkan-headers','LICENSE.txt')]:
        shutil.copy2(WORK/component/filename, dest/('recorddy-stream-' + component + '-LICENSE.txt'))
    print('Auxiliar portable: ' + str(dest))

def build():
    vswhere = pathlib.Path('C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe')
    vs = pathlib.Path(subprocess.check_output([str(vswhere), '-latest', '-property', 'installationPath'], text=True).strip())
    cmake = vs / 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'
    ninja = vs / 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe'
    shutil.copytree(WORK / 'glslang', WORK / 'ncnn/glslang', dirs_exist_ok=True)
    # No se copia el driver de esta PC ni se necesita instalar el SDK de Vulkan.
    header = (WORK / 'vulkan-headers/include/vulkan/vulkan_core.h').read_text()
    exports = sorted(set(re.findall(r'VKAPI_CALL (vk\w+)\(', header)))
    (WORK / 'vulkan.def').write_text('LIBRARY vulkan-1.dll\nEXPORTS\n' + '\n'.join(exports) + '\n', encoding='ascii')
    args = [str(cmake), '-S', str(ROOT / 'native/rife-stream'), '-B', str(WORK / 'build'), '-G', 'Ninja',
            '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_POLICY_VERSION_MINIMUM=3.5',
            '-DCMAKE_MAKE_PROGRAM=' + str(ninja), '-DRIFE_SRC=' + str(WORK / 'rife/src'),
            '-DNCNN_SRC=' + str(WORK / 'ncnn'), '-DVulkan_INCLUDE_DIR=' + str(WORK / 'vulkan-headers/include'),
            '-DVulkan_LIBRARY=' + str(WORK / 'vulkan.lib')]
    batch = WORK / 'build.cmd'
    batch.write_text('@echo off\nchcp 65001 >nul\ncall "' + str(vs / 'VC/Auxiliary/Build/vcvars64.bat') + '"\n'
        + 'if errorlevel 1 exit /b 1\n'
        + 'lib /nologo /def:"' + str(WORK / 'vulkan.def') + '" /machine:x64 /out:"' + str(WORK / 'vulkan.lib') + '"\n'
        + subprocess.list2cmdline(args) + '\nif errorlevel 1 exit /b 1\n'
        + subprocess.list2cmdline([str(cmake), '--build', str(WORK / 'build'), '--parallel', '4']) + '\n', encoding='utf-8')
    subprocess.run(['cmd', '/d', '/c', str(batch)], check=True)

if __name__ == '__main__': main()
