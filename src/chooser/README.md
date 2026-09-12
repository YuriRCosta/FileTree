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

This module is the request-validation slice. Portal transport, caller
watching, isolated browser windows and desktop registration are not wired
yet. No desktop role is activated by this code. Live browser upload and
parented-window behavior require those integrations before task 3.1/3.2
can pass.

Runnable check: compile `cargo test --locked --test chooser_requests --no-run`
and execute the resulting test binary in the assigned VM. The runtime lane
uses only harness A, SSH port 2422.
