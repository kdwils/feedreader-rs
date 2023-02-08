# feedreader

Simple feedreader that templates HTML and stores feeds + their articles. 

Created with the intention learning rust for fun

# TODO
- [ ] Automatic feed updates, currently can only manually refreshed a feed
- [ ] ci + repo for cd to deploy to home cluster
- [ ] a better solution for storing data, currently just connects to a postgres instance
- [x] fix error handling for scenarios that are not related to database errors
- [ ] clean up html templating
    - [ ] svgs everywhere is ugly
    - [ ] sketch up my own ui design
- [ ] the way I am spawning a tokio thread probably is not correct
    - [ ] research best practice, probably do in main fn

# resources used
* https://github.com/kasuboski/feedreader basically a copy of this,used as a template + referred to this when stuck
* https://brunoscheufler.com/blog/2022-01-01-paginating-large-ordered-datasets-with-cursor-based-pagination to implement forwards + backwards pagination with a cursor