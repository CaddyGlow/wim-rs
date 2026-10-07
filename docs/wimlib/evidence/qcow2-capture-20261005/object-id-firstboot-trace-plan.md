# Object-ID first-boot loss investigation

The standard-setup-first-reboot-metadata.json observation proves actual standard
Setup preserves all 64 object-ID bytes, including nonzero birth fields, at the
first requested reboot before the target OS starts. The later V6 observation
shows the original 16 bytes retained but all extended 48 bytes zeroed. Both
DISM apply controls also preserve the full buffer. This brackets the change
between target first boot and the later observation; it does not identify a
Windows component or justify accepting the loss. Current V12 source still
contains the nonzero challenge, and its clean installed baseline also loses
the extended bytes. A dedicated scratch file accepted the extension setter
and retained all64 bytes after supported shutdown and strict offline inspection.
On a writable child, a subsequent normal reboot again cleared all48 extension
bytes while preserving ObjectId16 and file identity24. The GET-only observer
reported no API error. This independently reproduces loss after OOBE, without
rerunning Setup or creating ObjectIDs during observation. See
[v12-root-object-id-normal-reboot-proof.json](v12-root-object-id-normal-reboot-proof.json).

Microsoft documents [FILE_OBJECTID_BUFFER](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-file_objectid_buffer)
as ObjectId plus BirthVolumeId/BirthObjectId/DomainId or opaque ExtendedInfo.
The domain field is reserved and must be zero; that does not require the two
nonzero birth fields to become zero. The protocol
[FSCTL_SET_OBJECT_ID_EXTENDED request](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/757bc8fa-f24f-44cf-86dc-adb2c113589d)
treats the extended 48 bytes as opaque and describes no NTFS content rule.
The [Win32 setter](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_object_id_extended)
changes extended data without changing ObjectId. Thus an extended-only setter
is a testable API hypothesis consistent with the observed pattern, not proof
that Windows issued it. Deletion/recreation with the same ObjectId and a zero
extended buffer, or an internal NTFS operation, remain competing hypotheses.

[Distributed link tracking](https://learn.microsoft.com/en-us/windows/win32/fileio/distributed-link-tracking-and-object-identifiers)
uses object IDs, with a client service managing local tracking. This makes its
activity worth observing alongside Setup/specialization processes. The docs do
not say it clears birth fields at first boot. Neither those docs nor generic
Sysprep identity reset documentation proves this is an intended transformation.
No service should be disabled to test this observation.

Prepare a future disposable child from a preserved preboot target baseline,
leaving the source and baseline immutable. Record exact full64 values, both
hardlink file IDs and link count, selected volume identity and whole fixture
hashes using the strict offline reader before boot. Retain the parent hash.
A boot observer may write its own trace files on this disposable branch but
must not change services, policies, object IDs or target file contents.
[Process Monitor](https://learn.microsoft.com/en-us/sysinternals/downloads/procmon)
provides boot logging, filesystem events and thread stacks. Installing its boot
observer changes instrumentation on the branch and must be recorded separately;
it is not a claim that the instrumented image is byte-identical to the control.
Use documented boot logging and preserve the native PML, process tree, module
versions, symbol provenance, and any dropped-event indicators. Do not apply
irreversible destructive filters at acquisition.

Analyze events involving both fixture paths, their parent and volume, FSCTL
operations and the object-ID index. Keep source and target path identities
separate. Correlate service start events and Setup/Sysprep timestamps with
successful FSCTL_SET_OBJECT_ID_EXTENDED, FSCTL_SET_OBJECT_ID,
FSCTL_DELETE_OBJECT_ID and CREATE_OR_GET requests if the trace exposes them.
A Procmon operation name or stack alone may not expose the 48-byte input;
record that limitation. Positive attribution needs a successful operation,
resolved file identity, caller/stack and a correlated before/after full64 change,
with the supplied data captured if available through a supported diagnostic
interface. A silent trace cannot exonerate a component when collection coverage
or event payloads are insufficient.

Query only FSCTL_GET_OBJECT_ID on no-follow handles at earliest available
setup instrumentation, OOBE entry, desktop readiness and after the first normal
reboot. Never use CREATE_OR_GET for observation: it can create metadata. Record
all64 bytes, actual returned length, status, file ID and UTC/boot-relative time;
close/reopen between observations. A supported shutdown followed by a clean
strict offline read confirms the final actual on-disk fields. A query timeline
brackets the transition but does not prove which caller changed it. Keep a
second uninstrumented disposable control to assess observer effects.

The release requirement remains full source-exact 64-byte installed metadata.
A future restoration experiment is a different action and needs independent
validation; this plan makes no restoration, policy waiver or intended-loss claim.
