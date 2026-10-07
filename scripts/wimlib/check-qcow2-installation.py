#!/usr/bin/env python3
"""Standard Windows Setup gate for an exact native-captured ISO and a blank VM.

prepare never starts a guest. start requires a hash-matched captured-media report.
observe, servicing, reboot, and recovery only act on the recorded disposable VM.
No DISM application, disk cloning, hardware bypass, or recovery repair is used.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import time
import xml.etree.ElementTree as ET

from windows_guest import WindowsGuest

HERE = Path(__file__).resolve().parent
REPOSITORY = HERE.parent.parent
spec = importlib.util.spec_from_file_location(
    "qcow2_boot_helpers", HERE.parents[2] / "windows-uup/scripts/native-copy-boot-smoke.py"
)
boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boot)
METADATA = runpy.run_path(str(HERE / "check-windows-metadata-reliability.py"))
NTFS = runpy.run_path(str(HERE / "check-windows-ntfs-metadata.py"))


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(2 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def save(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def checked(result):
    result.pop("stdout_base64", None)
    if result["exit"] != 0 or result.get("stdout_truncated"):
        raise RuntimeError(json.dumps(result))
    return result


def read_json(path):
    value = json.loads(Path(path).read_text(encoding="utf-8-sig"))
    if isinstance(value, dict) and isinstance(value.get("stdout"), str):
        if value.get("exit") != 0:
            raise ValueError("source observation exited unsuccessfully")
        return json.loads(value["stdout"])
    return value


def guest_ps(guest, code, timeout=300):
    return guest.execute(
        r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        ["-NoProfile", "-NonInteractive", "-Command",
         "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); " + code],
        timeout=timeout,
    )


IDENTITY = r"""
$ErrorActionPreference='Stop';
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class FirmwareProbe{[DllImport("kernel32.dll",SetLastError=true)]static extern bool GetFirmwareType(out uint type);public static uint Type(){uint type;if(!GetFirmwareType(out type))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return type;}}';
$cv=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion';
$user=Get-LocalUser -Name INSTALLATION_ACCOUNT;$os=Get-CimInstance Win32_OperatingSystem;
$marker=Get-ItemProperty 'HKLM:\SOFTWARE\NativeDiskCapture';
$ready=Test-Path -LiteralPath C:\qcow2-install-gate-ready.txt;
$setup=Get-ItemProperty 'HKLM:\SYSTEM\Setup';
$desktop=@(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue|Select-Object Id,SessionId,UserName);
$tpm=Get-Tpm;$re=& reagentc.exe /info 2>&1;$reExit=$LASTEXITCODE;
$tpmHardware=Get-CimInstance -Namespace root/CIMV2/Security/MicrosoftTpm -ClassName Win32_Tpm;
$encryption=Get-BitLockerVolume -MountPoint C:;
$partitions=@(Get-Partition -DiskNumber 0|ForEach-Object{
 [PSCustomObject]@{Number=$_.PartitionNumber;Guid=[string]$_.Guid;GptType=[string]$_.GptType;Offset=$_.Offset;Size=$_.Size;Letter=[string]$_.DriveLetter}
});
$disks=@(Get-Disk|ForEach-Object{[PSCustomObject]@{Number=$_.Number;Guid=[string]$_.Guid;UniqueId=[string]$_.UniqueId;PartitionStyle=[string]$_.PartitionStyle;Signature=$_.Signature}});
$rexml=$null;if(Test-Path -LiteralPath C:\Windows\System32\Recovery\ReAgent.xml){
 [xml]$rexml=[IO.File]::ReadAllText('C:\Windows\System32\Recovery\ReAgent.xml');
}
$location=[regex]::Match(($re -join "`n"),'Windows RE location:\s*(.+)').Groups[1].Value.Trim();
$reHash='';if($location){$reHash=[NtfsProbe]::Hash($location+'\Winre.wim');}
[ordered]@{
 Ready=$ready;Build=[int]$cv.CurrentBuildNumber;UBR=[int]$cv.UBR;Edition=$cv.EditionID;
 MachineGuid=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid;
 UserSid=$user.SID.Value;MachineSid=($user.SID.Value -replace '-[0-9]+$','');ComputerName=$env:COMPUTERNAME;
 LastBoot=$os.LastBootUpTime.ToUniversalTime().ToString('o');
 Marker=$marker.Marker;Application=((& cmd.exe /c '"C:\Program Files\NativeDiskCapture\marker.cmd"') -join "`n").Trim();
 BITS=(Get-CimInstance Win32_Service -Filter "Name='BITS'").StartMode;
 SecureBoot=(Confirm-SecureBootUEFI);TpmPresent=$tpm.TpmPresent;TpmReady=$tpm.TpmReady;
 TpmSpecVersion=$tpmHardware.SpecVersion;TpmEnabled=$tpmHardware.IsEnabled_InitialValue;TpmActivated=$tpmHardware.IsActivated_InitialValue;
 VolumeEncryptionState=[string]$encryption.VolumeStatus;VolumeEncryptionPercentage=$encryption.EncryptionPercentage;
 SetupState=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State').ImageState;
 SetupInProgress=(Get-ItemProperty 'HKLM:\SYSTEM\Setup').SystemSetupInProgress;
 OobeInProgress=$setup.OOBEInProgress;SetupType=$setup.SetupType;SetupPhase=$setup.SetupPhase;Desktop=$desktop;
 FirmwareType=[FirmwareProbe]::Type();
 Partitions=$partitions;Disks=$disks;PartitionStyle=[string](Get-Disk -Number 0).PartitionStyle;
 RecoveryInfo=($re -join "`n");RecoveryExit=$reExit;RecoveryHash=$reHash;
 RecoveryLocation=$location;RecoveryXml=$(if($rexml){$rexml.OuterXml});
 RecoveryBcd=$(if($rexml){[string]$rexml.WindowsRE.WinreBCD.id});
 RecoveryGuid=$(if($rexml){[string]$rexml.WindowsRE.WinreLocation.guid});
 RecoveryOffset=$(if($rexml){[string]$rexml.WindowsRE.WinreLocation.offset});
 RecoveryRelativePath=$(if($rexml){[string]$rexml.WindowsRE.WinreLocation.path});
 RecoveryLocationId=$(if($rexml){[string]$rexml.WindowsRE.WinreLocation.id});
}|ConvertTo-Json -Depth 6 -Compress
"""


def source_inventory_command():
    """Read-only fixture baseline; source owner must run before its frozen capture."""
    inventory = METADATA["INVENTORY"].replace("NORMALIZE_ROOT", r"C:\NativeDiskCapture")
    inventory = inventory.replace("ROOT", r"C:\NativeDiskCapture")
    return METADATA["HELPER"] + no_follow_ntfs_helper() + inventory


def no_follow_ntfs_helper():
    helper = NTFS["HELPER"]
    expected = 'CreateFile(p,write?0xC0000000u:0x80000000u,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero)'
    if helper.count(expected) != 1:
        raise ValueError('shared NTFS handle flags changed; revalidate no-follow runtime probe')
    return helper.replace(expected, expected.replace('0x02000000', '0x02200000'))


def metadata_command():
    # Do not recurse through reparses. The shared native probe opens metadata and
    # streams with backup semantics and OPEN_REPARSE_POINT, including link ADS.
    command = source_inventory_command()
    expected = "Streams=@([MetadataProbe]::Streams($p)|Sort-Object Name);Reparse="
    if command.count(expected) != 1:
        raise ValueError('shared fixture inventory changed; revalidate all-row EA/ObjectID queries')
    command = command.replace(
        expected,
        "Streams=@([MetadataProbe]::Streams($p)|Sort-Object Name);EA=[NtfsProbe]::EA($p);ObjectID=[NtfsProbe]::ObjectID($p,$false);Reparse=",
    )
    return command


def normalize_rows(rows):
    result = {}
    groups = {}
    for original in rows:
        row = json.loads(json.dumps(original))
        info = row["Info"]
        identity = info.pop("Identity")
        groups.setdefault(identity, []).append(row["Path"])
        row["Streams"].sort(key=lambda stream: stream["Name"])
        for stream in row["Streams"]:
            stream["Hash"] = stream["Hash"].replace("-", "").lower()
        result[row["Path"]] = row
    for paths in groups.values():
        for path in paths:
            result[path]["HardlinkPaths"] = sorted(paths)
    return result


def complete_ea_object_id_baseline(rows):
    if not rows:
        return False
    for row in rows:
        for field in ('EA', 'ObjectID'):
            value = row.get(field)
            if not isinstance(value, str):
                return False
            if value and re.fullmatch(r'[0-9A-Fa-f]{2}(?:-[0-9A-Fa-f]{2})*', value) is None:
                return False
        if row['ObjectID'] and len(row['ObjectID'].split('-')) != 64:
            return False
    return True


def metadata_differences(before, after):
    source, target = normalize_rows(before), normalize_rows(after)
    differences = []
    for path in sorted(source.keys() | target.keys()):
        a, b = source.get(path), target.get(path)
        if a is None or b is None:
            differences.append({"path": path, "source": a, "installed": b})
            continue
        # The independent full-volume DISM gate established these two metadata
        # transformations. Retain raw observations; report all other differences.
        a = json.loads(json.dumps(a))
        attrs = a["Info"]["Attributes"]
        if b["Info"]["Attributes"] == (attrs & ~128) | 32:
            a["Info"]["Attributes"] = b["Info"]["Attributes"]
        if not b["Short"] and a["Short"].casefold() == path.rsplit("\\", 1)[-1].casefold():
            a["Short"] = ""
        # An older baseline without EA/objectID cannot prove their restoration.
        for field in ("EA", "ObjectID"):
            if field not in a:
                b = dict(b)
                b.pop(field, None)
        if a != b:
            differences.append({"path": path, "source": a, "installed": b})
    return differences


def snapshot(state, label):
    channel = boot.Qmp(state["vm_state"] + "/qmp.sock")
    try:
        screen = Path(state["evidence_state"]) / (label + ".ppm")
        channel.request("screendump", {"filename": str(screen)})
        image = boot.png_from_ppm(screen)
        details = {"status": channel.request("query-status"), "blocks": channel.request("query-block"),
                   "tpm_devices": channel.request('query-tpm'),
                   "screenshot": str(image), "screenshot_sha256": sha(image)}
        save(Path(state["evidence_state"]) / (label + "-qmp.json"), details)
        return details
    finally:
        channel.close()


def prepare(args):
    answer = HERE / "qcow2-install-autounattend.xml"
    validate_installation_account(answer, args.interactive_user)
    state_dir = args.state.resolve()
    state_dir.mkdir(parents=True, exist_ok=False)
    vm_state = state_dir / "vm-state"
    if vm_state.exists():
        raise RuntimeError("installation instance already exists; choose a new explicit instance")
    base_disk = Path.home() / ".vm-base" / args.base / "disk.qcow2"
    info = json.loads(subprocess.check_output(["qemu-img", "info", "--output=json", str(base_disk)]))
    mapping = json.loads(subprocess.check_output(["qemu-img", "map", "--output=json", str(base_disk)]))
    if info["virtual-size"] != 64 * 1024 ** 3 or not mapping or not all(part.get("zero") for part in mapping):
        raise RuntimeError("answer file requires an independently confirmed empty 64 GiB base")
    seed_dir = state_dir / "seed"
    seed_dir.mkdir()
    shutil.copyfile(answer, seed_dir / "autounattend.xml")
    seed = state_dir / "seed.iso"
    subprocess.run([args.seed_builder, "-quiet", "-J", "-r", "-V", "QCOW2GATE", "-o", str(seed), str(seed_dir)], check=True)
    state = {"schema": 1, "scope": "standard Windows Setup from native captured ISO; no manual image apply",
             "instance": args.instance, "base": args.base, "base_disk": str(base_disk), "base_info": info,
             "base_map": mapping, "base_sha256": sha(base_disk), "vm_state": str(vm_state),
             "evidence_state": str(state_dir), "seed": str(seed), "seed_sha256": sha(seed),
             "answer_sha256": sha(answer), "seed_builder": args.seed_builder,
             "interactive_user": args.interactive_user, "steps": {}}
    save(state_dir / "installation.json", state)
    print(state_dir / "installation.json", flush=True)


def validate_installation_account(answer, user):
    """Require supported account provisioning and known matching auto-logon."""
    if re.fullmatch(r'[A-Za-z0-9_.-]{1,20}', user) is None:
        raise ValueError('invalid installation account')
    ns = {'u': 'urn:schemas-microsoft-com:unattend'}
    tree = ET.parse(answer)
    shell = tree.find("u:settings[@pass='oobeSystem']/u:component[@name='Microsoft-Windows-Shell-Setup']", ns)
    if shell is None:
        raise ValueError('answer lacks supported OOBE shell settings')
    autologon = shell.find('u:AutoLogon', ns)
    accounts = shell.findall('u:UserAccounts/u:LocalAccounts/u:LocalAccount', ns)
    account = next((a for a in accounts if a.findtext('u:Name', namespaces=ns) == user), None)
    if (autologon is None or account is None
            or autologon.findtext('u:Username', namespaces=ns) != user
            or autologon.findtext('u:Enabled', namespaces=ns) != 'true'
            or 'Administrators' not in (account.findtext('u:Group', namespaces=ns) or '').split(';')):
        raise ValueError('OOBE requires a newly provisioned administrator matching the observed auto-logon account')
    password = account.findtext('u:Password/u:Value', namespaces=ns)
    if (not password or password != autologon.findtext('u:Password/u:Value', namespaces=ns)
            or account.findtext('u:Password/u:PlainText', namespaces=ns) != 'true'
            or autologon.findtext('u:Password/u:PlainText', namespaces=ns) != 'true'):
        raise ValueError('OOBE auto-logon credential must match the provisioned validation account')


def start(args, state):
    if args.iso is None or args.media_report is None:
        raise ValueError("start requires --iso and --media-report")
    iso = args.iso.resolve(strict=True)
    report = read_json(args.media_report)
    expected = report.get("iso_sha256")
    if not expected or sha(iso) != expected:
        raise RuntimeError("installation ISO differs from the native producer report")
    if report.get("image_count") != 1 or report.get("winre_verified", 0) < 1 or report.get("warnings"):
        raise RuntimeError("requires one verified captured image and verified embedded WinRE with no metadata warnings")
    if Path(state["vm_state"]).exists() or "start" in state["steps"]:
        raise RuntimeError("start never resets or resumes an existing VM")
    if sha(state["seed"]) != state["seed_sha256"] or sha(state["base_disk"]) != state["base_sha256"]:
        raise RuntimeError("prepared seed or immutable blank base changed")
    environment = dict(os.environ, VM_INSTALL=str(iso), VM_SEED=state["seed"], VM_STATE=state["vm_state"],
                       VM_OS="windows", VM_DISK_BUS="sata", VM_NET="none", VM_QGA="1", VM_SECUREBOOT="1",
                       VM_TPM="1", VM_MEM="8192", VM_CPUS="4", VM_VNC="auto")
    command = ["vm-run", "--base", state["base"], state["instance"]]
    with (args.state / "vm-run.stdout").open("wb") as out, (args.state / "vm-run.stderr").open("wb") as err:
        process = subprocess.Popen(command, env=environment, cwd=REPOSITORY, stdout=out, stderr=err, start_new_session=True)
    state["iso"] = str(iso)
    state["iso_sha256"] = expected
    state["media_report"] = str(args.media_report.resolve())
    state["media_report_sha256"] = sha(args.media_report)
    state["steps"]["start"] = {"launcher_pid": process.pid, "command": command, "hardware": {
        key: environment[key] for key in ("VM_OS", "VM_DISK_BUS", "VM_NET", "VM_QGA", "VM_SECUREBOOT", "VM_TPM", "VM_MEM", "VM_CPUS")}}
    save(args.state / "installation.json", state)
    deadline = time.monotonic() + 45
    while not Path(state["vm_state"], "qmp.sock").exists():
        if process.poll() is not None:
            raise RuntimeError((args.state / "vm-run.stderr").read_text())
        if time.monotonic() > deadline:
            raise TimeoutError("launcher remains live but QMP not yet available; inspect before retrying")
        time.sleep(0.25)
    channel = boot.Qmp(Path(state["vm_state"]) / "qmp.sock")
    try:
        for _ in range(40):
            channel.request("send-key", {"keys": [{"type": "qcode", "data": "spc"}], "hold-time": 30})
            time.sleep(0.25)
    finally:
        channel.close()
    state["steps"]["initial_boot"] = snapshot(state, "setup-start")
    save(args.state / "installation.json", state)


def checkpoint(args, state):
    """Preserve stopped baseline bytes; subsequent writes go to a new child."""
    if 'baseline' in state['steps']:
        raise RuntimeError('baseline already exists; never replace retained evidence')
    if not state['steps'].get('observation', {}).get('gates', {}).get('setup_oobe_completed'):
        raise RuntimeError('baseline checkpoint requires observed complete OOBE desktop')
    vm_state = Path(state['vm_state'])
    if any((vm_state / name).exists() for name in ('qmp.sock', 'qga.sock')):
        raise RuntimeError('checkpoint requires completed orderly guest shutdown')
    image = vm_state / 'overlay.qcow2'
    info = json.loads(subprocess.check_output(['qemu-img', 'info', '--output=json', str(image)]))
    if info.get('dirty-flag') or info.get('format-specific', {}).get('data', {}).get('corrupt'):
        raise RuntimeError('QCOW image is not clean; preserve failure without checkpoint mutation')
    if sha(state['base_disk']) != state['base_sha256']:
        raise RuntimeError('immutable blank base changed')
    expected = sha(image)
    directory = args.state / 'baseline'
    directory.mkdir(exist_ok=False)
    baseline = directory / 'overlay.qcow2'
    image.rename(baseline)
    try:
        subprocess.run(['qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', str(baseline), str(image)], check=True)
    except BaseException:
        # Roll back only the newly created owned child; never remove baseline.
        if image.exists():
            image.unlink()
        baseline.rename(image)
        raise
    if sha(baseline) != expected:
        raise RuntimeError('baseline hash changed; do not resume')
    shutil.copy2(vm_state / 'OVMF_VARS.fd', directory / 'OVMF_VARS.fd')
    shutil.copytree(vm_state / 'tpm', directory / 'tpm')
    record = {'path': str(baseline), 'sha256': expected, 'qemu_info': info,
              'authorized_backings': [state['base_disk']], 'child_path': str(image),
              'child_initial_sha256': sha(image), 'created_utc': time.time(),
              'observation_retained': True, 'ntfs_clean_validation': 'pending independent offline native open',
              'firmware_sha256': sha(directory / 'OVMF_VARS.fd')}
    state['steps']['baseline'] = record
    save(args.state / 'baseline' / 'baseline.json', record)
    save(args.state / 'installation.json', state)


def resume(args, state):
    baseline = state['steps'].get('baseline')
    if not baseline or 'resume' in state['steps']:
        raise RuntimeError('requires one retained baseline and no previous resume request')
    if not args.baseline_native_proof:
        raise ValueError('resume requires --baseline-native-proof from completed native offline checks')
    proof = read_json(args.baseline_native_proof)
    if (proof.get('image_sha256') != baseline['sha256'] or proof.get('partition') != 3
            or proof.get('exit') != 0 or not proof.get('command') or not proof.get('artifacts')):
        raise RuntimeError('native baseline proof does not bind successful offline validation')
    for artifact in proof['artifacts']:
        if sha(artifact['path']) != artifact['sha256']:
            raise RuntimeError('native baseline validation artifact changed')
    vm_state = Path(state['vm_state'])
    if any((vm_state / name).exists() for name in ('qmp.sock', 'qga.sock')):
        raise RuntimeError('guest already running; do not launch duplicate')
    if sha(baseline['path']) != baseline['sha256'] or sha(state['base_disk']) != state['base_sha256']:
        raise RuntimeError('frozen baseline or original base changed')
    if sha(vm_state / 'overlay.qcow2') != baseline['child_initial_sha256']:
        raise RuntimeError('unbooted servicing child changed')
    environment = dict(os.environ, VM_STATE=state['vm_state'], VM_OS='windows', VM_DISK_BUS='sata',
                       VM_NET='none', VM_QGA='1', VM_SECUREBOOT='1', VM_TPM='1',
                       VM_MEM='8192', VM_CPUS='4', VM_VNC='auto')
    environment.pop('VM_INSTALL', None)
    environment.pop('VM_SEED', None)
    command = ['vm-run', '--base', state['base'], state['instance']]
    with (args.state / 'vm-resume.stdout').open('xb') as out, (args.state / 'vm-resume.stderr').open('xb') as err:
        process = subprocess.Popen(command, env=environment, cwd=REPOSITORY,
                                   stdout=out, stderr=err, start_new_session=True)
    state['steps']['resume'] = {'launcher_pid': process.pid, 'command': command,
                               'baseline_native_proof_sha256': sha(args.baseline_native_proof),
                               'no_install_media_or_seed': True, 'requested_utc': time.time()}
    save(args.state / 'installation.json', state)


def recovery_destination(identity, source):
    """Resolve Windows' disk GUID + byte offset, then corroborate GLOBALROOT."""
    guid = identity.get("RecoveryGuid", "").strip("{}").lower()
    disks = [disk for disk in identity.get("Disks", [])
             if disk["Guid"].strip("{}").lower() == guid]
    try:
        offset = int(identity.get("RecoveryOffset", ""))
    except (ValueError, TypeError):
        offset = -1
    partitions = [part for part in identity["Partitions"] if part["Offset"] == offset]
    location = identity.get("RecoveryLocation", "")
    match = re.fullmatch(r"\\\\\?\\GLOBALROOT\\device\\harddisk(\d+)\\partition(\d+)(\\.+)", location, re.I)
    disk = disks[0] if len(disks) == 1 else None
    partition = partitions[0] if len(partitions) == 1 else None
    relative = identity.get("RecoveryRelativePath", "")
    source_disk_guids = {d['Guid'].strip('{}').lower() for d in source.get('Disks', [])}
    source_partition_guids = {p['Guid'].strip('{}').lower() for p in source.get('Partitions', [])}
    partition_type = partition['GptType'].strip('{}').lower() if partition else ''
    recovery_partition = partition_type == 'de94bba4-06d1-4d40-a16a-bfd50179d6ac'
    os_partition = bool(partition_type == 'ebd0a0a2-b9e5-4433-87c0-68b6b72699c7'
                        and partition['Letter'].upper() == 'C')
    passes = bool(disk and partition and match and disk["Number"] == 0
                  and disk["PartitionStyle"] == "GPT"
                  and int(match.group(1)) == disk["Number"]
                  and int(match.group(2)) == partition["Number"]
                  and match.group(3).rstrip("\\").casefold() == relative.rstrip("\\").casefold()
                  and (recovery_partition or os_partition)
                  and source_disk_guids and source_partition_guids and guid not in source_disk_guids
                  and partition['Guid'].strip('{}').lower() not in source_partition_guids
                  and relative.casefold().startswith('\\recovery\\')
                  and '..' not in relative.split('\\'))
    return {"passes": passes, "separate_recovery_partition_passes": bool(passes and recovery_partition),
            "disk": disk, "partition": partition,
            "xml_disk_guid": guid, "xml_partition_offset": offset,
            "xml_relative_path": relative, "registered_location": location,
            "partition_role": 'recovery' if recovery_partition else 'os' if os_partition else 'unsupported',
            "semantics": "WinreLocation GUID identifies new GPT disk; byte offset identifies its OS or recovery partition"}


def winre_equivalence(path, source_hash, target_hash):
    """Recheck full independent artifacts; never accept a report's pass boolean."""
    result = {'passes': False, 'errors': [], 'proof_path': str(path)}
    try:
        if path.stat().st_size > 1024 * 1024:
            raise ValueError('equivalence report exceeds bound')
        proof = json.loads(path.read_text())
        if proof.get('schema') != 1 or proof.get('kind') != 'winre-full-equivalence':
            raise ValueError('unsupported WinRE equivalence schema')
        if proof['source_sha256'] != source_hash or proof['target_sha256'] != target_hash:
            raise ValueError('proof does not bind captured source and observed destination WinRE')
        if proof['differences'] != []:
            raise ValueError('independent comparison contains differences')
        if type(proof['entry_count']) is not int or not 0 < proof['entry_count'] <= 250000:
            raise ValueError('invalid or excessive complete entry count')
        if type(proof['stream_count']) is not int or not 0 < proof['stream_count'] <= 1000000:
            raise ValueError('invalid or excessive complete stream count')
        artifacts = proof['artifacts']
        required = {side + '_' + kind for side in ('source', 'target')
                    for kind in ('wim', 'manifest', 'xml', 'kernel', 'verify')}
        if not required.issubset(artifacts) or len(artifacts) > 32:
            raise ValueError('missing or excessive independent artifacts')
        files = {}
        for name, item in artifacts.items():
            file = Path(item['path'])
            limit = 4 * 1024 ** 3 if name.endswith('_wim') else 64 * 1024 ** 2
            if not file.is_absolute() or not file.is_file() or file.stat().st_size > limit or sha(file) != item['sha256']:
                raise ValueError(name + ': missing or changed artifact')
            files[name] = file
        if artifacts['source_wim']['sha256'] != source_hash or artifacts['target_wim']['sha256'] != target_hash:
            raise ValueError('WIM artifacts do not bind the observed containers')
        manifests, identities = {}, {}
        for side in ('source', 'target'):
            file = files[side + '_manifest']
            if file.stat().st_size > 64 * 1024 * 1024:
                raise ValueError('WinRE manifest exceeds bound')
            manifest = json.loads(file.read_text())
            if manifest.get('schema') != 1 or manifest.get('kind') != 'winre-content-manifest':
                raise ValueError('unsupported full manifest schema')
            entries = manifest['entries']
            canonical = {}
            streams_count = 0
            for entry in entries:
                key = bytes.fromhex(entry['path_utf16_le'])
                if len(key) % 2 or key in canonical:
                    raise ValueError('invalid or duplicate full manifest path')
                streams = []
                stream_keys = set()
                if type(entry['attributes']) is not int or not 0 <= entry['attributes'] <= 0xffffffff:
                    raise ValueError('invalid entry attributes')
                for stream in entry['streams']:
                    name = bytes.fromhex(stream['name_utf16_le'])
                    identity = (stream['kind'], name)
                    if len(name) % 2 or identity in stream_keys or stream['kind'] not in ('DATA', 'REPARSE'):
                        raise ValueError('invalid or duplicate stream identity')
                    if type(stream['size']) is not int or stream['size'] < 0 or re.fullmatch('[0-9a-f]{64}', stream['sha256']) is None:
                        raise ValueError('invalid stream length or SHA256')
                    stream_keys.add(identity)
                    streams.append((stream['kind'], name.hex(), stream['size'], stream['sha256']))
                if not entry['attributes'] & (0x10 | 0x400) and ('DATA', b'') not in stream_keys:
                    raise ValueError('ordinary file lacks its complete unnamed stream')
                canonical[key.hex()] = (entry['attributes'], sorted(streams))
                streams_count += len(streams)
            if not entries or len(entries) != proof['entry_count'] or streams_count != proof['stream_count']:
                raise ValueError('full manifest counts differ')
            manifests[side] = canonical
            xml = ET.parse(files[side + '_xml']).getroot()
            if len(xml.findall('IMAGE')) != 1:
                raise ValueError('WinRE must contain exactly one image')
            image = xml.find('IMAGE')
            root_keys = [key for key in canonical
                         if bytes.fromhex(key).decode('utf-16-le', 'surrogatepass') == '/']
            if len(root_keys) != 1 or not canonical[root_keys[0]][0] & 0x10:
                raise ValueError('full manifest must include one explicit directory root')
            if int(image.findtext('DIRCOUNT')) + int(image.findtext('FILECOUNT')) + 1 != len(entries):
                raise ValueError('full manifest does not cover XML entry count')
            windows = image.find('WINDOWS')
            if files[side + '_kernel'].stat().st_size > 64 * 1024 * 1024:
                raise ValueError('decoded kernel exceeds bound')
            kernel = files[side + '_kernel'].read_bytes()
            pe = int.from_bytes(kernel[0x3c:0x40], 'little')
            if kernel[:2] != b'MZ' or kernel[pe:pe+4] != b'PE\0\0' or kernel[pe+4:pe+6] != b'\x64\x86':
                raise ValueError('WinRE kernel is not AMD64 PE')
            observed = {'architecture': int(windows.findtext('ARCH')),
                        'build': int(windows.findtext('VERSION/BUILD')),
                        'revision': int(windows.findtext('VERSION/SPBUILD')),
                        'kernel_sha256': artifacts[side + '_kernel']['sha256']}
            if observed != proof[side + '_identity'] or observed['architecture'] != 9:
                raise ValueError('WinRE XML/kernel identity differs from proof')
            kernel_entries = [value for key, value in canonical.items()
                              if bytes.fromhex(key).decode('utf-16-le', 'surrogatepass').replace('\\', '/').casefold()
                              == '/windows/system32/ntoskrnl.exe']
            if len(kernel_entries) != 1 or ('DATA', '', len(kernel), observed['kernel_sha256']) not in kernel_entries[0][1]:
                raise ValueError('decoded kernel does not match full content manifest')
            identities[side] = observed
            verify_log = files[side + '_verify'].read_text()
            if 'was successfully verified.' not in verify_log:
                raise ValueError('missing independent full WIM verification completion')
        if manifests['source'] != manifests['target'] or identities['source'] != identities['target']:
            raise ValueError('WinRE full entry/stream/attribute or kernel/build identity differs')
        result.update(passes=True, proof_sha256=sha(path), artifacts=artifacts,
                      entry_count=proof['entry_count'], stream_count=proof['stream_count'], identity=identities['target'])
    except (OSError, ValueError, KeyError, TypeError, ET.ParseError) as error:
        result['errors'].append(str(error))
    return result


def observe(args, state):
    if args.source_preparation is None or args.source_metadata is None:
        raise ValueError("observe requires --source-preparation and --source-metadata")
    guest = WindowsGuest(Path(state["vm_state"]) / "qga.sock")
    interactive_user = state.get('interactive_user', 'deploy')
    if re.fullmatch(r'[A-Za-z0-9_.-]{1,20}', interactive_user) is None:
        raise ValueError('invalid recorded installation account')
    identity_run = guest_ps(guest, METADATA["HELPER"] + NTFS["HELPER"] + IDENTITY.replace('INSTALLATION_ACCOUNT', interactive_user))
    save(args.state / "installed-identity-run.json", identity_run)
    checked(identity_run)
    identity = json.loads(identity_run["stdout"])
    inventory_run = guest_ps(guest, metadata_command())
    save(args.state / "installed-metadata-run.json", inventory_run)
    checked(inventory_run)
    rows = json.loads(inventory_run["stdout"])
    source = read_json(args.source_preparation)
    if args.capture_report is None:
        raise ValueError("observe requires --capture-report for exact captured WinRE and image identity")
    capture = read_json(args.capture_report)
    if args.source_identity is None:
        raise ValueError("observe requires --source-identity for SID evidence")
    source.update(read_json(args.source_identity))
    source_rows = read_json(args.source_metadata)
    extras = {row['Path']: row for row in source_rows
              if isinstance(row.get('EA'), str) and isinstance(row.get('ObjectID'), str)}
    if args.source_fixture:
        fixture = read_json(args.source_fixture)
        extras.update({"\\" + entry["Name"]: entry for entry in fixture
                       if isinstance(entry.get("EA"), str) and isinstance(entry.get("ObjectID"), str)})
    for row in source_rows:
        if row["Path"] in extras:
            row["EA"] = extras[row["Path"]]["EA"]
            row["ObjectID"] = extras[row["Path"]]["ObjectID"]
    expected_build = int(source["Build"])
    source_sid = source.get("MachineSid") or re.sub(r"-[0-9]+$", "", source.get("UserSid") or source.get("DeploySid", ""))
    recovery = recovery_destination(identity, source)
    equivalence = None
    recovery_hash = identity['RecoveryHash'].replace('-', '').lower()
    if args.winre_equivalence:
        equivalence = winre_equivalence(args.winre_equivalence, capture['winre_sha256'].lower(), recovery_hash)
    differences = metadata_differences(source_rows, rows)
    gates = {
        "setup_oobe_completed": bool(identity["Ready"] and identity["SetupInProgress"] == 0
                                     and identity["OobeInProgress"] == 0 and identity["SetupType"] == 0
                                     and identity["SetupState"] == "IMAGE_STATE_COMPLETE"
                                     and any(p["SessionId"] > 0 and str(p.get('UserName') or '').casefold().endswith('\\' + interactive_user.casefold())
                                             for p in identity["Desktop"])),
        "fresh_identity": bool(source_sid and identity["MachineSid"] != source_sid
                               and identity["MachineGuid"] != source["MachineGuid"]
                               and identity['MachineGuid'] != source.get('PreGeneralizationMachineGuid')),
        "target_version": identity["Build"] == expected_build and identity["UBR"] == int(source["UBR"]) and identity["Edition"] == source["Edition"],
        "application_registry_service": identity["Marker"] == "qcow2-native-1005" and identity["Application"] == "qcow2-native-1005" and identity["BITS"] == "Manual",
        "secure_boot_tpm": bool(identity["SecureBoot"] and identity["TpmPresent"] and identity["TpmReady"]),
        "guest_tpm2": bool(str(identity.get('TpmSpecVersion') or '').split(',')[0].strip() == '2.0'
                           and identity['TpmEnabled'] and identity['TpmActivated']),
        "fully_decrypted": identity['VolumeEncryptionState'] == 'FullyDecrypted' and identity['VolumeEncryptionPercentage'] == 0,
        "metadata": not differences,
        "ea_object_id_baseline": complete_ea_object_id_baseline(source_rows),
        "winre_registered": bool(identity["RecoveryExit"] == 0 and "Enabled" in identity["RecoveryInfo"] and identity["RecoveryHash"] and recovery["passes"]),
        "winre_separate_recovery_partition": recovery["separate_recovery_partition_passes"],
        "winre_payload": recovery_hash == capture['winre_sha256'].lower() or bool(equivalence and equivalence['passes']),
        "fresh_gpt_layout": identity["PartitionStyle"] == "GPT" and len(identity["Partitions"]) == 4,
        "source_iso_base_preserved": sha(state["iso"]) == state["iso_sha256"] and sha(state["base_disk"]) == state["base_sha256"],
    }
    source_disk_guids = {d['Guid'].strip('{}').lower() for d in source.get('Disks', [])}
    source_partition_guids = {p['Guid'].strip('{}').lower() for p in source.get('Partitions', [])}
    destination_guids = {p['Guid'].strip('{}').lower() for p in identity['Partitions']}
    gates['fresh_efi_layout'] = bool(identity['FirmwareType'] == 2 and source_disk_guids and source_partition_guids
                                     and len(destination_guids) == 4 and destination_guids.isdisjoint(source_partition_guids)
                                     and all(d['Guid'].strip('{}').lower() not in source_disk_guids for d in identity['Disks'])
                                     and any(p['Number'] == 1 and p['GptType'].strip('{}').lower()
                                             == 'c12a7328-f81f-11d2-ba4b-00a0c93ec93b' for p in identity['Partitions']))
    # ReAgent XML GUID/offset must reference the newly partitioned destination,
    # and its BCD object must differ from any recorded pre-generalize object.
    source_bcd = source.get("RecoveryBcd") or re.search(r"identifier:\s*([0-9a-f-]{36})", source.get("Recovery", ""), re.I)
    if hasattr(source_bcd, "group"):
        source_bcd = source_bcd.group(1)
    target_bcd = identity['RecoveryBcd'].strip('{}').lower()
    gates["fresh_recovery_bcd"] = bool(source_bcd and re.fullmatch('[0-9a-f-]{36}', target_bcd)
                                       and target_bcd != '00000000-0000-0000-0000-000000000000'
                                       and target_bcd != str(source_bcd).strip('{}').lower())
    if "reboot_request" in state["steps"]:
        gates["servicing_reboot_completed"] = identity["LastBoot"] != state["steps"]["reboot_request"]["before_boot"]
        feature_run = checked(guest_ps(guest, "[string](Get-WindowsOptionalFeature -Online -FeatureName TelnetClient).State"))
        gates["serviced_feature_after_reboot"] = feature_run["stdout"].strip() == "Enabled"
    observation = {"identity": identity, "raw_identity": identity_run, "source_preparation": source,
                   "recovery_destination": recovery,
                   "winre_equivalence": equivalence,
                   "capture_report_sha256": sha(args.capture_report),
                   "source_preparation_sha256": sha(args.source_preparation), "source_identity_sha256": sha(args.source_identity), "source_fixture_sha256": sha(args.source_fixture) if args.source_fixture else None, "source_metadata_sha256": sha(args.source_metadata),
                   "source_recovery_bcd_provenance": source.get('RecoveryBcdProvenance', 'Retained source Windows observation'),
                   "ea_object_id_baseline_paths": sorted(extras),
                   "metadata": rows, "raw_metadata": inventory_run, "metadata_differences": differences,
                   "gates": gates, "passes": all(gates.values()), "boot": snapshot(state, "installed")}
    state["steps"].setdefault("observation_history", []).append(observation)
    state["steps"]["observation"] = observation
    save(args.state / "installation.json", state)
    print(json.dumps({"gates": gates, "metadata_differences": len(differences)}, indent=2), flush=True)


SERVICING = r"""
$ErrorActionPreference='Stop';$r='C:\qcow2-install-gate';New-Item -ItemType Directory -Force $r|Out-Null;
$feature=Get-WindowsOptionalFeature -Online -FeatureName TelnetClient;
$before=[string]$feature.State;
if($before -notin @('Enabled','Disabled')){throw ('TelnetClient requires locally available stable feature payload, observed '+$before)};
& dism.exe /English /Online /Cleanup-Image /ScanHealth ('/LogPath:'+$r+'\dism-scan.log') 2>&1|Out-File ($r+'\scan.txt');$scan=$LASTEXITCODE;
& sfc.exe /verifyonly 2>&1|Out-File ($r+'\sfc.txt');$sfc=$LASTEXITCODE;
if($scan -ne 0 -or $sfc -ne 0 -or
   -not ([IO.File]::ReadAllText($r+'\scan.txt').Contains('No component store corruption detected')) -or
   -not ([IO.File]::ReadAllText($r+'\sfc.txt').Replace([string][char]0,'').Contains('did not find any integrity violations'))){
 throw 'Health gate failed; feature servicing was not attempted'
};
if($before-eq 'Enabled'){& dism.exe /English /Online /Disable-Feature /FeatureName:TelnetClient /NoRestart ('/LogPath:'+$r+'\feature-disable.log') 2>&1|Out-File ($r+'\disable.txt');$disable=$LASTEXITCODE;}else{$disable=0;}
& dism.exe /English /Online /Enable-Feature /FeatureName:TelnetClient /All /NoRestart /LimitAccess ('/LogPath:'+$r+'\feature-enable.log') 2>&1|Out-File ($r+'\enable.txt');$enable=$LASTEXITCODE;
[ordered]@{ScanExit=$scan;Scan=([IO.File]::ReadAllText($r+'\scan.txt'));SfcExit=$sfc;Sfc=([IO.File]::ReadAllText($r+'\sfc.txt'));FeatureBefore=$before;DisableExit=$disable;EnableExit=$enable;FeatureAfter=[string](Get-WindowsOptionalFeature -Online -FeatureName TelnetClient).State;BeforeBoot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}|ConvertTo-Json -Compress
"""


def servicing(args, state):
    if not state["steps"].get("observation", {}).get("passes"):
        raise RuntimeError("servicing requires a passing installed-target observation")
    if "servicing" in state["steps"]:
        raise RuntimeError("servicing already recorded; inspect instead of repeating mutation")
    guest = WindowsGuest(Path(state["vm_state"]) / "qga.sock")
    result = guest_ps(guest, SERVICING, timeout=1800)
    state['steps']['servicing'] = {'raw': result, 'passes': False}
    save(args.state / 'installation.json', state)
    checked(result)
    values = json.loads(result["stdout"])
    values["Sfc"] = values["Sfc"].replace("\0", "")
    gates = {"scan_health": values["ScanExit"] == 0 and "No component store corruption detected" in values["Scan"],
             "sfc": values["SfcExit"] == 0 and "did not find any integrity violations" in values["Sfc"],
             "offline_feature_servicing": values["DisableExit"] in (0, 3010) and values["EnableExit"] in (0, 3010) and values["FeatureAfter"] in ("Enabled", "EnablePending")}
    state["steps"]["servicing"] = {"result": values, "raw": result, "gates": gates, "passes": all(gates.values())}
    save(args.state / "installation.json", state)
    print(json.dumps(gates), flush=True)


def reboot(args, state):
    if not state['steps'].get('servicing', {}).get('passes'):
        raise RuntimeError('servicing reboot requires all servicing gates to pass')
    if "reboot_request" in state["steps"]:
        raise RuntimeError("reboot request already exists; inspect or observe the existing guest")
    guest = WindowsGuest(Path(state["vm_state"]) / "qga.sock")
    before = checked(guest_ps(guest, "(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')"))["stdout"].strip()
    state["steps"]["reboot_request"] = {"before_boot": before, "requested_utc": time.time()}
    save(args.state / "installation.json", state)
    checked(guest.execute(r"C:\Windows\System32\shutdown.exe", ["/r", "/t", "5"]))


def recovery(args, state):
    observation = state["steps"].get("observation", {})
    if (not observation.get("passes")
            or observation.get("gates", {}).get("winre_separate_recovery_partition") is not True):
        raise RuntimeError("WinRE boot requires passing target and separate recovery-partition registration gates")
    if "recovery_request" in state["steps"]:
        raise RuntimeError("recovery request already exists; capture and inspect the existing boot")
    guest = WindowsGuest(Path(state["vm_state"]) / "qga.sock")
    request = checked(guest.execute(r"C:\Windows\System32\reagentc.exe", ["/boottore"]))
    state["steps"]["recovery_request"] = {"raw": request, "requested_utc": time.time(), "visual_boot_pass": False}
    save(args.state / "installation.json", state)
    checked(guest.execute(r"C:\Windows\System32\shutdown.exe", ["/r", "/t", "5"]))


def recovery_confirm(args, state):
    request = state['steps'].get('recovery_request')
    if not request or request.get('visual_boot_pass'):
        raise RuntimeError('requires an existing unconfirmed WinRE boot request')
    if not args.recovery_observation:
        raise ValueError('requires --recovery-observation describing the inspected WinRE screen')
    if len(args.recovery_observation.strip()) < 20:
        raise ValueError('record concrete recovery UI observations, not only a pass label')
    evidence = snapshot(state, 'winre-boot-confirmed')
    request.update(visual_boot_pass=True, visual_observation=args.recovery_observation,
                   boot_evidence=evidence, confirmed_utc=time.time())
    save(args.state / 'installation.json', state)



def collect_logs(args, state):
    guest = WindowsGuest(Path(state["vm_state"]) / "qga.sock")
    command = r"""
$ErrorActionPreference='Stop';$r='C:\qcow2-install-gate';New-Item -ItemType Directory -Force $r|Out-Null;
$items=New-Object 'Collections.Generic.List[string]';
foreach($p in @('C:\Windows\Panther','C:\Windows\System32\Sysprep\Panther','C:\vm\firstboot.log','C:\Windows\Logs\CBS\CBS.log','C:\Windows\Logs\DISM\dism.log','C:\Windows\System32\Recovery\ReAgent.xml','C:\Windows\Logs\ReAgent')){if(Test-Path -LiteralPath $p){$items.Add($p)}};
Get-ChildItem -LiteralPath $r -File|Where-Object{$_.Extension-ne '.zip'}|ForEach-Object{$items.Add($_.FullName)};
Add-Type -AssemblyName System.IO.Compression;Add-Type -AssemblyName System.IO.Compression.FileSystem;
$destination=$r+'\logs.zip';if(Test-Path -LiteralPath $destination){Remove-Item -LiteralPath $destination};
$zip=[IO.Compression.ZipFile]::Open($destination,[IO.Compression.ZipArchiveMode]::Create);
$errors=New-Object 'Collections.Generic.List[object]';$paths=New-Object 'Collections.Generic.List[string]';
try{foreach($item in $items){
 $files=@(Get-Item -LiteralPath $item -Force);if($files[0].PSIsContainer){$files=@(Get-ChildItem -LiteralPath $item -Force -Recurse -File)};
 foreach($file in $files){
  $input=$null;$output=$null;
  try{$input=[IO.File]::Open($file.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite-bor [IO.FileShare]::Delete));
   $entry=$zip.CreateEntry($file.FullName.Substring(3).Replace('\','/'));$output=$entry.Open();$input.CopyTo($output);$paths.Add($file.FullName);
  }catch{$errors.Add([PSCustomObject]@{Path=$file.FullName;Error=$_.Exception.Message})}
  finally{if($output){$output.Dispose()};if($input){$input.Dispose()}}
 }
}}finally{$zip.Dispose()};
[ordered]@{Bytes=(Get-Item -LiteralPath $destination).Length;Files=$paths.ToArray();Errors=$errors.ToArray()}|ConvertTo-Json -Depth 4 -Compress
"""
    result = checked(guest_ps(guest, command))
    logs = args.state / "installed-logs.zip"
    logs.write_bytes(guest.get(r"C:\qcow2-install-gate\logs.zip"))
    state["steps"]["logs"] = {"path": str(logs.resolve()), "sha256": sha(logs), "size": logs.stat().st_size, "raw": result}
    save(args.state / "installation.json", state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "start", "checkpoint", "resume", "capture", "observe", "servicing", "reboot", "recovery", "recovery-confirm", "logs", "stop"))
    parser.add_argument("--state", required=True, type=Path)
    parser.add_argument("--instance", default="qcow2-installed-1005")
    parser.add_argument("--base", default="win11-24h2-installer")
    parser.add_argument('--interactive-user', default='capturegate')
    parser.add_argument("--seed-builder", default="genisoimage")
    parser.add_argument("--iso", type=Path)
    parser.add_argument("--media-report", type=Path)
    parser.add_argument("--capture-report", type=Path)
    parser.add_argument("--source-preparation", type=Path)
    parser.add_argument("--source-metadata", type=Path)
    parser.add_argument("--source-identity", type=Path)
    parser.add_argument("--source-fixture", type=Path)
    parser.add_argument("--label", default="screen")
    parser.add_argument('--recovery-observation')
    parser.add_argument('--winre-equivalence', type=Path)
    parser.add_argument('--baseline-native-proof', type=Path)
    args = parser.parse_args()
    args.state = args.state.resolve()
    if args.action == "prepare":
        prepare(args)
        return
    state = json.loads((args.state / "installation.json").read_text())
    allowed_vm_states = {
        (REPOSITORY / "vm-state" / args.instance).resolve(),
        args.state / "vm-state",
    }
    if state["instance"] != args.instance or Path(state["vm_state"]).resolve() not in allowed_vm_states:
        raise RuntimeError("explicit instance does not match the recorded disposable VM")
    actions = {"start": start, "checkpoint": checkpoint, "resume": resume, "observe": observe, "servicing": servicing, "reboot": reboot, "recovery": recovery, "recovery-confirm": recovery_confirm, "logs": collect_logs}
    if args.action in actions:
        actions[args.action](args, state)
    elif args.action == "capture":
        if re.fullmatch(r"[a-zA-Z0-9_-]+", args.label) is None:
            raise ValueError("screenshot label must be a simple name")
        state["steps"][args.label] = snapshot(state, args.label)
        save(args.state / "installation.json", state)
    elif args.action == "stop":
        subprocess.run(
            ["vm-stop", state["instance"]], cwd=REPOSITORY,
            env=dict(os.environ, VM_STATE=state["vm_state"]), check=True,
        )
        state["steps"]["stop"] = {"base_preserved": sha(state["base_disk"]) == state["base_sha256"]}
        save(args.state / "installation.json", state)


if __name__ == "__main__":
    main()
