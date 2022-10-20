# mqtt-ping

Tool that is useful for debugging publish forwards of a broker, 
starts two clients, one that subscribes and one that subscribes
then measures the time between sending, and reception.

first ping will always take more time due to creating a path, 
thus the first time is treated differently, and not used in the average 
and median calculations

## Screenshot

<img src="https://github.com/totalkrill/mqttping-rs/blob/master/screenshot.png/?raw=true">

## installation

`cargo install mqtt-ping`

## Usage

basic:

`mqtt-ping --broker mqtt://localhost`

see the help for more

`mgtt-ping --help`

