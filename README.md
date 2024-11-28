# ZK-utreexo

This project aims to implement SP-1 powered zk-circuit for updating a utreexo accumulator. Currently, the project have the following components:

- `circuit` - zk-circuit for updating a utreexo accumulator.
- `server` - a bitcoin bridge node, which fetches block data from a bitcoin node for further passage it into the circuit.
- `rustreexo` - a rust implementation of utreexo accumulator with some zk-friendly modifications.
- `input-generator` - a tool to take fetched data from the `server` and prepare it for the `circuit`.

## Status

Project is WIP, and we don't have all components working together yet. However, we already can manually run the `server` and run `circuit` with prepared data.


## Aknowledgements
`server` and `rustreexo` are based on the work of Davidson-Souza and mit-dci:
https://github.com/mit-dci/rustreexo
https://github.com/Davidson-Souza/rpc-utreexo-bridge
