# feedreader

Simple feedreader that templates HTML and stores feeds + their articles. 

# TODO
- [ ] Automatic feed updates, currently can only manually refreshed a feed
- [ ] ci + repo for cd to deploy to home cluster
- [ ] a better solution for storing data, currently just connects to a postgres instance
- [ ] fix error handling for scenarios that are not related to database errors
    - [ ] research best practices
- [ ] clean up html templating
    - [ ] svgs everywhere is ugly
    - [ ] sketch up my own ui design
- [ ] the way I am spawning a tokio thread probably is not correct
    - [ ] research best practice, probably do in main fn

# resources used
* https://github.com/kasuboski/feedreader basically a clone
* https://brunoscheufler.com/blog/2022-01-01-paginating-large-ordered-datasets-with-cursor-based-pagination to implement forwards + backwards pagination with a cursor