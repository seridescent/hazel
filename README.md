# hazel

a dead-simple service to run nix installables for http servers from github.

this is just a side quest to get automated staging and production deploys on my homelab for a different side project,
a full stack app.

## why would i do this?

i wanted a way to mess with cloud claude code while being cheap about hosting.
there are tons of platforms to run fullstack apps on that have nice github integrations,
but my homelab is more capable, isolated to my tailnet, and has zero marginal cost. 
thus, i just needed to solve the github integration part, because the fun of prompting from my phone would really be diminished
if i had to manually deploy test builds.

ultimately, writing this seemed preferred to wrangling docker and probably some other things.

one nice thing i get out of this approach though is being able to specify both hazel's and the target 
app's deployment in nix installables. containerization is basically fine on servers but
is annoying on my macbook, so using a nix installables approach is simpler *for me*.

## why you should not use this

i can't stress enough that you really should not use this in the general case.
ask your favorite LLM how to do self-hosted gitops CICD and you'll probably get 
some pointers on kubernetes or some other more put-together tools. or just use a platform.

since i'm the only consumer and i have a very focused use case, this service specifically
elides enforced isolation. there's nothing stopping a process started by hazel from 
doing anything the hazel process has permission to do anywhere on your computer.
i might add some extra process sandboxing in the future but it's not really a priority.

again, you really should not use this.

## why this approach is still interesting/valuable

i do find value in having a tiny bit of glue code that i understand in its entirety
instead of (possibly) stitching together higher-level components.

i wouldn't call this vibe-coded per say, as i understand all the code here;
it's really not a lot of code!! 
i would not have written this without an LLM to iterate with though because i'm lazy.
now that code is cheaper, i think homelab-runners and self-hosters should consider
doing this more because vertical integration makes it easy to, frankly, elide a lot of 
defensive programming that isn't so relevant. 
i can afford to not worry about a lot of things *because* of the specialized use case
and the network isolation!

for now, i do recommend this general approach for personal automatic deploys. 
github's app api is easy to work with and has a generous rate limit,
and wrapping nix makes it fun and malleable.
