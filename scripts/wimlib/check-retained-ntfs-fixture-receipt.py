#!/usr/bin/env python3
"""Validate Windows creation receipt; actual offline tests remain separate gates."""
import argparse
import json
from pathlib import Path


def validate(receipt):
    if receipt.get('completed') is not True or receipt.get('error'):
        raise ValueError('Windows fixture creation did not complete')
    expected={'V12-CASE-FIXTURE':'V','V12-ADS-FIXTURE':'W'}
    disks=receipt['disks']
    if len(disks)!=2 or len({d['number'] for d in disks})!=2:
        raise ValueError('fixture disks missing or not distinct')
    if {(d['serial'],d['requested_letter']) for d in disks}!=set(expected.items()):
        raise ValueError('requires exact case and ADS disk serial/letter pairs')
    for d in disks:
        if d['serial'] not in expected or d['requested_letter']!=expected[d['serial']] or d['number']==0 or d['is_boot'] or d['is_system'] or d['virtual_bytes']!=536870912 or d['before_style']!='RAW':
            raise ValueError('unsafe or unexpected fixture disk identity')
        if d['after_style']!='MBR' or len(d['after_partitions'])!=1:
            raise ValueError('requires single NTFS partition1')
        p=d['after_partitions'][0]
        if p['PartitionNumber']!=1 or p['DiskNumber']!=d['number'] or p['DriveLetter']!=d['requested_letter'] or p['Offset']<=0 or p['Size']<=0 or p['Offset']+p['Size']>d['virtual_bytes']:
            raise ValueError('partition identity or bounds differ')
        v=d['after_volume']
        if v['FileSystem']!='NTFS' or v['DriveLetter']!=d['requested_letter'] or v['Size']<=0 or v['Size']>p['Size']:
            raise ValueError('actual formatted NTFS volume evidence differs')
    case=receipt['case']
    if case['path']!=r'V:\case' or case['control_path']!=r'V:\control' or not case['empty'] or case['global_policy_changed'] or case['control_attributes']!=16 or any(case[key]!=0 for key in ['enable_exit','query_exit','control_query_exit']):
        raise ValueError('case/control creation contract failed')
    ads=receipt['ads']
    if ads['unnamed_byte']!=170:
        raise ValueError('unnamed file byte differs')
    if ads['path']!=r'W:\OddFixture\many-ads.bin' or ads['partition']!=1 or ads['count']!=160 or len(ads['verified_streams'])!=160:
        raise ValueError('ADS fixture count/path/layout failed')
    for index, row in enumerate(ads['verified_streams']):
        if row!={'name':f'stream-{index:03d}','length':1,'byte':index}:
            raise ValueError('incorrect ADS byte/name/length')
    actual={r['Stream']:r['Length'] for r in ads['windows_stream_enumeration'] if r['Stream'].startswith('stream-')}
    if actual!={f'stream-{index:03d}':1 for index in range(160)}:
        raise ValueError('Windows stream enumeration differs')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('receipt',type=Path);args=parser.parse_args()
    validate(json.loads(args.receipt.read_bytes()))
    print('Windows creation receipt valid; clean native offline tests still required')
