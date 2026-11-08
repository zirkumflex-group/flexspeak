struct Placement {
    rect: vec4<f32>, clip: vec4<f32>, fade: vec4<u32>,
    viewport: vec4<f32>, image: vec4<f32>, opacity: vec4<f32>,
};
@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u: Placement;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) p: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index:u32)->Vertex {
    let corner=vec2<f32>(f32(index&1u),f32(index>>1u));
    let p=u.rect.xy+corner*u.rect.zw;
    let local=p-u.viewport.zw;
    var out:Vertex;
    out.position=vec4<f32>(local.x/u.viewport.x*2.0-1.0,1.0-local.y/u.viewport.y*2.0,0.0,1.0);
    out.p=p;
    return out;
}
// libyuv YuvI601Constants / YuvPixel: fixed-point, including its rounding bias.
fn rgb_at(pixel:vec2<i32>)->vec3<f32> {
    let p=clamp(pixel,vec2<i32>(0),vec2<i32>(u.image.xy)-vec2<i32>(1));
    let y=i32(round(textureLoad(y_plane,p,0).r*255.0));
    let cb=i32(round(textureLoad(u_plane,p/2,0).r*255.0))-128;
    let cr=i32(round(textureLoad(v_plane,p/2,0).r*255.0))-128;
    let l=((y*257*18997)>>16)-1160;
    let rgb=vec3<f32>(clamp(vec3<i32>((l+102*cr)>>6,(l-25*cb-52*cr)>>6,(l+128*cb)>>6),vec3<i32>(0),vec3<i32>(255)))/255.0;
    if u.image.w<0.5 { return select(pow((rgb+0.055)/1.055,vec3<f32>(2.4)),rgb/12.92,rgb<=vec3<f32>(0.04045)); }
    return rgb;
}
@fragment fn fs(in:Vertex)->@location(0) vec4<f32> {
    let c=cos(u.image.z); let s=sin(u.image.z);
    // The old Pixels path fits first, then rotates the fitted picture about its centre.
    let fit=min(u.rect.z/u.image.x,u.rect.w/u.image.y);
    let q=in.p-u.rect.xy-u.rect.zw*0.5;
    let uv=vec2<f32>(c*q.x+s*q.y,-s*q.x+c*q.y)/fit+u.image.xy*0.5;
    if any(uv<vec2<f32>(0.0)) || any(uv>=u.image.xy) { discard; }
    // Each screen pixel covers `footprint` source pixels a side.
    let footprint=1.0/fit;
    var rgb:vec3<f32>;
    if footprint<=1.0 {
        // Enlarged or 1:1: linear filtering like the RGBA atlas, after nearest 2x2 chroma.
        let pos=uv-0.5; let base=vec2<i32>(floor(pos)); let t=fract(pos);
        rgb=mix(mix(rgb_at(base),rgb_at(base+vec2<i32>(1,0)),t.x),mix(rgb_at(base+vec2<i32>(0,1)),rgb_at(base+vec2<i32>(1,1)),t.x),t.y);
    } else if footprint<=8.0 {
        // Shrunk: the exact area average of the source pixels the screen pixel covers, each
        // weighted by its overlap, as libyuv's kFilterBox scaled it on the CPU. Uneven sampling
        // would turn fine text and one-pixel lines into moire.
        let lo=uv-vec2<f32>(footprint*0.5); let hi=uv+vec2<f32>(footprint*0.5);
        var sum=vec3<f32>(0.0); var total=0.0;
        for (var y=i32(floor(lo.y)); y<i32(ceil(hi.y)); y++) {
            let wy=min(hi.y,f32(y+1))-max(lo.y,f32(y));
            for (var x=i32(floor(lo.x)); x<i32(ceil(hi.x)); x++) {
                let w=wy*(min(hi.x,f32(x+1))-max(lo.x,f32(x)));
                sum+=rgb_at(vec2<i32>(x,y))*w; total+=w;
            }
        }
        rgb=sum/total;
    } else {
        // Shrunk more than 8x (small thumbnails of very large sources): 8x8 evenly spread taps.
        let step=footprint/8.0;
        let first=uv-vec2<f32>(footprint*0.5-step*0.5);
        var sum=vec3<f32>(0.0);
        for (var j=0;j<8;j++) {
            for (var i=0;i<8;i++) {
                sum+=rgb_at(vec2<i32>(floor(first+vec2<f32>(f32(i),f32(j))*step)));
            }
        }
        rgb=sum/64.0;
    }
    let a=clip_alpha(in.p,u.clip,u.fade)*u.opacity.x;
    return vec4<f32>(rgb*a,a);
}

