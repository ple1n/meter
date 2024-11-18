clone embassy main branch to ../embassy

## design considerations

### device to host communication

there isnt a need to use ethernet, and the stack didnt work out on stm32f103 due to memory constraint. It couldnt be put on it at all.

instead, run point to point protocol over usb_serial. 

