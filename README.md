# sp1-utreexo

This project aims to implement SP-1 powered zk-circuit for updating a utreexo accumulator. Currently, the project have the following components:

- `circuit` - zk-circuit for updating a utreexo accumulator.
- `server` - a bitcoin bridge node, which fetches block data from a bitcoin node for further passage it into the circuit.
- `rustreexo` - a rust implementation of utreexo accumulator with some zk-friendly modifications.
- `input-generator` - a tool to take fetched data from the `server` and prepare it for the `circuit`.

## Status

Project is WIP and here is the list of things we already have:

- [x] we already have a zk-circuit for updating a utreexo accumulator
- [x] we have a server which fetches block data from a bitcoin node and updates utreexo outside circuit
- [x] we have code to prepare data for the circuit
- [x] we implemented some robust performance optimizations for circuit

TODO:
- [ ] integrate circuit with the server
- [ ] add endpoints to server for getting utreexo proofs and utreexo roots for given height 


## How to run

From very high level there are two steps:
- run `server` following server/README.md and wait it to start processing blocks (it should be around a minute)
- run `python3 run-end-to-end.py` from the root of the project


## Aknowledgements
`server` and `rustreexo` are based on the work of Davidson-Souza and mit-dci:
https://github.com/mit-dci/rustreexo
https://github.com/Davidson-Souza/rpc-utreexo-bridge
