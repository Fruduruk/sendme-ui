# Sendme-ui
This program is an extension of the [sendme](https://github.com/n0-computer/sendme) project.

It is the same code but instead of a console you get an [egui](https://github.com/emilk/egui) interface.
That's it.

## Copy from the sendme readme
This is an example application using [iroh](https://crates.io/crates/iroh) with
the [iroh-blobs](https://crates.io/crates/iroh-blobs) protocol to send files and
directories over the internet.

It is also useful as a standalone tool for quick copy jobs.

Iroh will take care of hole punching and NAT traversal whenever possible,
and fall back to a relay if hole punching does not succeed.

Iroh-blobs will take care of [blake3](https://crates.io/crates/blake3) verified
streaming, including resuming interrupted downloads.

Sendme works with 256 bit node ids and is, therefore, location transparent. A ticket
will remain valid if the IP address changes. Connections are encrypted using
TLS.

# Usage

## Send side
This will create a temporary [iroh](https://crates.io/crates/iroh) node that
serves the content in the given file or directory. It will output a ticket that
can be used to get the data.

The provider will run until it is terminated using the cancel button. On termination, it
will delete the temporary directory.

This currently will create a temporary directory in the current directory. In
the future this won't be needed anymore.

### Receive side
This will download the data and create a file or directory named like the source
in the **specified directory**.

It will create a temporary directory in the current directory, download the data
(single file or directory), and only then move these files to the target
directory.

On completion, it will delete the temp directory.

All temp directories start with `.sendme-`.
