#!/usr/bin/env python3
"""Compare native XML parser and properties with original libwim public API."""
import argparse
import ctypes as c
from pathlib import Path
import struct
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--oracle', type=Path, required=True)
p.add_argument('--native', type=Path, required=True)
p.add_argument('--fixture', type=Path, required=True)
a = p.parse_args()
lib = c.CDLL(str(a.oracle.resolve()))
lib.wimlib_open_wim.argtypes = [c.c_char_p, c.c_int, c.POINTER(c.c_void_p)]
lib.wimlib_free.argtypes = [c.c_void_p]
lib.wimlib_set_image_property.argtypes = [c.c_void_p,c.c_int,c.c_char_p,c.c_char_p]
lib.wimlib_get_image_property.argtypes = [c.c_void_p,c.c_int,c.c_char_p]
lib.wimlib_get_image_property.restype = c.c_char_p
lib.wimlib_parse_and_write_xml_doc.argtypes = [c.c_char_p,c.POINTER(c.c_char_p)]
base=a.fixture.read_bytes()
xml_cases = [
    '<WIM><IMAGE INDEX="1"><NAME>A</NAME><NAME>B</NAME><DESCRIPTION>one\r\ntwo</DESCRIPTION></IMAGE></WIM>',
    '<WIM attr="&amp;"><IMAGE INDEX="1"><CUSTOM x="y">one<![CDATA[😀]]><!--c--><?pi?><Z/>two</CUSTOM></IMAGE></WIM>',
    '<WIM><IMAGE INDEX="+1"><ns:x>literal</ns:x></IMAGE></WIM>',
    '<WIM><IMAGE INDEX="1"><1invalid>allowed</1invalid></IMAGE></WIM>',
    '<WIM><IMAGE INDEX="1"><NAME>&#65;</NAME></IMAGE></WIM>',
    '<WIM><IMAGE INDEX="1"><NAME>&unknown;</NAME></IMAGE></WIM>',
    '<WIM><IMAGE INDEX="0"/></WIM>',
    '<WIM><IMAGE INDEX="2"/></WIM>',
    '<WIM><IMAGE INDEX="1"/><IMAGE INDEX="1"/></WIM>',
    '<WIM><ESD><ENCRYPTED/></ESD></WIM>',
    '<wrong/>',
    '<WIM>'+ '<A>'*50 + '</A>'*50 + '</WIM>',
]
count=0
with tempfile.TemporaryDirectory(prefix='wim-xml-diff-') as temp:
    temp=Path(temp)
    def make_wim(xml, images=1):
        raw=b'\xff\xfe'+xml.encode('utf-16le')
        xml_path=temp/'xml.bin';xml_path.write_bytes(raw)
        data=bytearray(base)
        struct.pack_into('<I',data,44,images)
        struct.pack_into('<QQQ',data,72,len(raw),len(data),len(raw))
        data.extend(raw)
        path=temp/'case.wim';path.write_bytes(data)
        return xml_path,path
    for xml in xml_cases:
        xml_path,path=make_wim(xml)
        handle=c.c_void_p();oracle=lib.wimlib_open_wim(bytes(path),0,c.byref(handle))
        native=subprocess.check_output([str(a.native.resolve()),str(xml_path)]).decode().split('\n',1)
        assert oracle==int(native[0]), (xml,oracle,native[0])
        if handle.value:lib.wimlib_free(handle)
        if oracle==0:
            normalized=c.c_char_p()
            assert lib.wimlib_parse_and_write_xml_doc(xml.encode(),c.byref(normalized))==0
            assert normalized.value.decode()==native[1],(xml, normalized.value,native[1])
        count+=1
    xml='<WIM><IMAGE INDEX="1"><NAME>A</NAME><FLAGS>old</FLAGS><NODE attr="x"><B/>t</NODE><L>one</L><L>two</L></IMAGE></WIM>'
    for image,key,value in [(1,'NAME','B'),(1,'FLAGS','new'),(1,'FLAGS',''),(1,'FLAGS',None),(1,'NODE','replacement'),(1,'L[2]','deux'),(1,'L[3]','third'),(1,'L[4]','bad'),(1,'L[1]',None),(1,'WINDOWS/LANGUAGE[2]','fr'),(1,'WINDOWS/LANGUAGE','en'),(1,'/NAME',None),(1,'/NAME','bad'),(0,'bad space',None),(0,'NAME',None),(1,'NAME/','trailing'),(1,'1BAD','bad'),(1,'X/1BAD','allowed'),(1,'CUSTOM','雪 😀 & < >'),(1,'CUSTOM','bad\x01'),(1,'FLAGS[0]','bad'),(1,'FLAGS[4294967296]','bad')]:
        xml_path,path=make_wim(xml)
        handle=c.c_void_p();assert lib.wimlib_open_wim(bytes(path),0,c.byref(handle))==0
        oracle=lib.wimlib_set_image_property(handle,image,key.encode(),None if value is None else value.encode())
        got=lib.wimlib_get_image_property(handle,image,key.encode())
        output=subprocess.check_output([str(a.native.resolve()),str(xml_path),str(image),key,'@none' if value is None else value]).decode().split('\n',2)
        expected='@none' if got is None else got.hex()
        assert oracle==int(output[0]) and expected==output[1],(image,key,value,oracle,expected,output[:2])
        lib.wimlib_free(handle);count+=1
    lib.wimlib_create_new_wim.argtypes=[c.c_int,c.POINTER(c.c_void_p)]
    lib.wimlib_add_empty_image.argtypes=[c.c_void_p,c.c_char_p,c.POINTER(c.c_int)]
    xml='<WIM><IMAGE INDEX="1"><NAME>A</NAME></IMAGE><IMAGE INDEX="2"><NAME>B</NAME></IMAGE></WIM>'
    for key,value in [('NAME','B'),('NAME','b'),('NAME[1]','B'),('NAME',None),('NAME','')]:
        handle=c.c_void_p();assert lib.wimlib_create_new_wim(0,c.byref(handle))==0
        for name in [b'A',b'B']:
            index=c.c_int();assert lib.wimlib_add_empty_image(handle,name,c.byref(index))==0
        oracle=lib.wimlib_set_image_property(handle,1,key.encode(),None if value is None else value.encode())
        got=lib.wimlib_get_image_property(handle,1,key.encode())
        xml_path=temp/'xml.bin';xml_path.write_bytes(b'\xff\xfe'+xml.encode('utf-16le'))
        output=subprocess.check_output([str(a.native.resolve()),str(xml_path),'1',key,'@none' if value is None else value]).decode().split('\n',2)
        expected='@none' if got is None else got.hex()
        assert oracle==int(output[0]) and expected==output[1],(key,value,oracle,expected,output[:2])
        lib.wimlib_free(handle);count+=1
print(f'{count} differential XML/public-property cases passed')
