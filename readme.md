This will be a sheet music viewing and annotation program with a major emphasis on 
accessibility for low-vision users.

Currently in proof-of-concept stage. Have confirmed that pen tablet drawing will work with
Freya via the Skia canvas, including pressure support. Needs optimization, but it will work
for now. Next I need to be able to layer images on top of each other, and hopefully render
PDF's as well. Ideally I will be able to do some image processing in shaders to speed up
finding a separating path between staves, but that is something that is only necessary
once per staff, and thus only relevant for importing and rearranging time, not rendering.
