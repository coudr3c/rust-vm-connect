# TODO List

## Must

- [X] Propagate RDP file info to RDP command
- [ ] Test on Windows using the actual Windows RDP command
- [ ] Remove hard coded variables such as VM targets

## Should

- [ ] Clean up logs
- [ ] Improve error handling, especially with threading
- [X] Allow stopping a connection
- [X] Stop tunnel connection on RDP thread stopped
- [X] Implement "terminate_session_with_error" that combines an error given with the potential terminate session error
- [ ] Integration tests
- [X] Let users choose port number

## Would

- [ ] If rdp file present, use it, else create it then use it
- [ ] For RDP : "Select connection profile" button, or dropdown with rdp file
