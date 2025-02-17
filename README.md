# Leaky

A content-addressable CMS built for the web. 

## What?

Leaky is a (potentially) distributed content management system that:
- is built on IPLD
- uses IPFS for content-addressed storage
- supports rich metadata attached to files
- supports user-defined schema validation per directory
- provides version control of content and metadata
- offers a simple git-like CLI interface

you can see a demo of leaky in action on my [personal website](https://krondor.org).
this site is consuming the remote i have deployed at [https://leaky.krondor.org](https://leaky.krondor.org).
you can check the current version of the site by visiting [https://leaky.krondor.org/api/v0/root](https://leaky.krondor.org/api/v0/root).
you can traverse the DAG of the site by visiting [my ipfs gateway](https://ipfs.io/ipfs/bafyr4ihw5rab74df5infj7hqgn6ijxyrk5oua6mqnaroskp3oaydt3bbmi).

### Modules

The system consists of three main components:

1. `leaky-cli`: Command line interface for managing content
2. `leaky-server`: Server for saving and serving content
3. `leaky-common`: Shared library containing the core data model and utilities

### Data Model

Leaky organizes content using:

- **Nodes**: Directory-like and File-like structures containing links to other nodes or data
- **Objects**: Metadata attached to nodes pointing to the data they contain
- **Schemas**: JSON definitions that validate object metadata at the node level
- **Manifests**: JSON files that describe the version history as well as an entrypoint to your content. This is described by a root CID that can be used to identify your content at any point in time.

Anyone with the root CID of your content can traverse and pull your content, either over IPFS or by talking to the `leaky-server`. `leaky-server` is a simple HTTP server will pull your manifest 
(from a local instance of kubo) and traverse the DAG to serve your content by its normal web2-looking path. For instance, if you have a node with the following structure:

```
/
  /about
    /index.md
  /contact
    /index.md
```

Then the following path will be accessible from either an IPFS gateway:

```
ipfs.io/ipfs/<root-cid>/about
```

or from the `leaky-server`:

```
https://<server-remote>/<root-cid>/about
```

Leaky also supports rich-metadata per file, which is stored at the node level and can be used to
store user-defined custom properties. For example, you could have a `properties` field that describes
the author of the file, the date it was created, etc.

```json
{
  "created_at": "2024-01-01",
  "updated_at": "2024-01-02",
  "properties": {
    "author": "John Doe",
    "title": "About Us"
  }
}
```

### Usage

You will need a local IPFS node running to use leaky. This must be accessible from localhost:5001.
Once you have kubo installed, you can start it with the following command:

```bash
ipfs daemon
```

With your remote deployed and configured, you can use the `leaky-cli` to manage your content.
Within an empty directory, you can initialize a new leaky project by running:

```bash
leaky-cli init --remote <remote-url>;
```

This will initialize your directory with `.leaky` directory describing your sync state with the remote.

You can then add and commit content to your project by running:

```bash
touch about.md
echo "About us" > about.md
leaky-cli add -v
leaky-cli push
```

This will add the `about.md` file to your project and push it to the remote. You can then pull and
view your content by running:

```bash
leaky-cli pull
```

### Development

To build the project, you can use the following command:

```bash
cargo build
```

To run the tests, you can use the following command:

```bash
cargo test
```

To format the code, you can use the following command:

```bash
cargo fmt
```

To lint the code, you can use the following command:

```bash
cargo clippy
```





