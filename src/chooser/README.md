# Native chooser request boundary

`Request` holds one immutable `Offer`, one optional overwrite identity and
one terminal `Outcome`. Keep a separate request per portal handle. Caller,
parent window, title, initial location, filename and filters belong to that
offer; they are never stored in ordinary browser preferences.

`choose` validates local paths, selection count, file/directory type,
read access and the selected caller filter before returning file URIs.
Filename globs use the platform `fnmatch`; MIME checks reuse the existing
GIO-backed content-type reader. A save destination is checked with the
existing filesystem validator. It is returned without creating or writing
anything. Existing symlink or special-file save targets are refused.

An existing save target returns `Decision::Overwrite`. A subsequent
approval applies only to that request, canonical parent/name and the same
device, inode and change timestamp. A replaced target requires another
confirmation. An unsuccessful attempt clears the preceding approval.
This protects the chooser's confirmation; the caller remains responsible
for its own later write and any changes after the chooser response.

`cancel` wins only once. A finished request rejects later selections and
cancellation cannot overwrite an accepted result. The portal adapter must
route user cancellation, Request.Close and caller disappearance to this
same terminal transition, unregister the request object, and close its
own window. It must serialize transitions for each handle and retain the
frontend portal's document-access mediation.

`transport` supplies the resident `chooser` command group: `offer` waits for
one result, `watch` returns a revision and offer snapshot, and `choose`,
`cancel` and `filter` serve the app consumer. At most sixteen offers exist.
Offer and watch commands must be classified as standing server requests so
waiting callers cannot exhaust completion/cancellation capacity. Cancellation
interrupts content-type probing; only each request's own validation is locked.
The immutable offer snapshot remains readable while validation runs.

The app consumer lives under `app/chooser/` and dynamically loads the real
Service/TreePane and picker footer. Each session shares the resident backend,
uses fresh in-memory navigation/layout state, and refuses unrelated writes.
`FILEBLADE_CHOOSER=1` enables the consumer for development qualification;
ordinary launches do not activate a desktop role. The final role switch owns
activation when task 3.3 lands.

The backend registers this command group on the resident authority.
Portal D-Bus registration, exported-parent association and real browser
upload remain unqualified; this slice does not complete tasks 3.1/3.2.

Runnable checks: compile `cargo test --locked --test chooser_requests
--test chooser_transport --no-run` and execute those binaries in the assigned
VM. `tests/vm/expectations/45-native-chooser.sh` checks live window/selection
isolation with qualification fixtures, independently of portal completion.
The runtime lane uses only harness A, SSH port 2422.

`46-native-chooser-resident.sh` uses actual socket callers against the freshly
built native fixture: filtered Open, existing/new Save, multiple and folder
selection, Escape cancellation and caller EOF. It checks returned URIs and
unchanged target/state/layout bytes. The native_authority case
`chooser_offers_leave_completion_capacity_and_cancel_on_real_caller_eof`
keeps sixteen offers on one connection, refuses a seventeenth, completes and
cancels requests on that connection, then proves EOF clears the others.
These checks require no portal registration and do not qualify foreign
parent association or a real browser upload.
