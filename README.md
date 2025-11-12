tondi-graph-inspector
=====================

TGI is comprised of four components:

* A postgres database
* A `processing` Tondi node (this connects to a Tondi node via RPC)
* An `api` REST server
* A `web` server

How the components interact:

* The `processing` node connects to the Tondi network via RPC and starts syncing blocks
* While it's syncing, it writes metadata about every block to the postgres database
* From the other end, the `web` server listens to http requests on some port
* When a user navigates their browser to that port, the `web` server serves the TGI clientside logic, which includes the UI
* The clientside logic calls the `api` REST server every so often
* The `api` REST server queries the postgres database and returns it to the clientside
* The clientside uses the response it received from the `api` REST server to update the UI

Development
-----------

For development, it's recommended to run TGI from within Docker

1. Make sure you have docker installed by running `docker --version`
2. Make sure you have docker-compose installed by running `docker-compose --version`
3. Define the following environment variables:
   1. POSTGRES_USER=username
   2. POSTGRES_PASSWORD=password
   3. POSTGRES_DB=database-name
   4. API_ADDRESS=localhost
   5. KATNIP_ADDRESS=localhost
   6. API_PORT=4575
   7. WEB_PORT=8080
   8. TONDID_VERSION=latest (this can be either a specific tondid commit hash or a tondid tag)
   9. TONDI_LIVE_ADDRESS=localhost
4. Run: `./docker-run.sh`

Deployment
----------

1. Deploy a postgres database instance in any way you desire. Note the address, port, username, password, and database name, since these will be required later
2. Build `processing`
   1. Make sure the Rust build environment is set up by running `rustc --version`
   2. Within the `processing` directory, run `cargo build --release`. This will produce an executable file named `tgi-processing` in `target/release/`
   3. Copy `tgi-processing` and `database` directory (also within the `processing` directory) to wherever you wish to run the node from
3. Build `api`
   1. Make sure the nodejs build environment is set up by running `npm version`
   2. Within the `api` directory, run: `npm install`
   3. Copy the entire `api` directory to wherever you wish to run the `api` server from
4. Build `web`
   1. Make sure the nodejs build environment is set up by running `npm version`
   2. Within the `web` directory, run: `npm install`
   3. Set the following environment variables:
      1. REACT_APP_API_ADDRESS=example.com:1234 (this is the public address of where your `api` server will be)
      2. REACT_APP_EXPLORER_ADDRESS=explorer.tondi.org
      3. REACT_APP_TONDI_LIVE_ADDRESS=tondi.live
   4. Within the `web` directory, run: `npm run build`
   5. Copy the entire `web` directory to wherever you wish to run the `web` server from
5. Run `processing`
   1. Navigate to wherever you copied `tgi-processing` and `database` to
   2. Set the following environment variables:
      1. POSTGRES_USER=username
      2. POSTGRES_PASSWORD=password
      3. POSTGRES_DB=database-name
      4. POSTGRES_HOST=database.example.com
      5. POSTGRES_PORT=5432
   3. Run: `tgi-processing --connection-string=postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@${POSTGRES_HOST}:${POSTGRES_PORT}/${POSTGRES_DB}?sslmode=disable --rpcserver=tondi-node.example.com:port`
6. Run `api`
   1. Navigate to wherever you copied `api` to
   2. Run: `npm run start`
7. Run `web`
   1. Navigate to wherever you copied `web` to
   2. Run: `npm install -g serve`
   3. Set the WEB_PORT environment variable to the port you wish to serve the TGI UI from
   4. Run: `serve --listen=${WEB_PORT}`
